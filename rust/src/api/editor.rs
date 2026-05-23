use flutter_rust_bridge::frb;
use ropey::Rope as RustRope;
use zed_sum_tree::{Item, SumTree, Summary};
use crate::api::rope::RopeBridge;
use std::collections::HashSet;
use std::mem;

#[flutter_rust_bridge::frb(init)]
pub fn init_app() {
    flutter_rust_bridge::setup_default_user_utils();
}

#[derive(Clone, Debug)]
pub struct LineBlock {
    pub len_chars: usize,
    pub height: f32,
    pub is_folded: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LineSummary {
    pub len_chars: usize,
    pub height: f32,
    pub lines: usize,
}

impl Summary for LineSummary {
    type Context<'a> = ();
    
    fn zero(_cx: ()) -> Self {
        Self::default()
    }
    
    fn add_summary(&mut self, other: &Self, _: ()) {
        self.len_chars += other.len_chars;
        self.height += other.height;
        self.lines += other.lines;
    }
}

impl Item for LineBlock {
    type Summary = LineSummary;
    fn summary(&self, _: ()) -> Self::Summary {
        LineSummary {
            len_chars: self.len_chars,
            height: if self.is_folded { 0.0 } else { self.height },
            lines: 1,
        }
    }
}

#[frb(opaque)]
pub struct LayoutMap {
    tree: SumTree<LineBlock>,
}

impl LayoutMap {
    #[frb(sync)]
    pub fn new() -> Self {
        Self {
            tree: SumTree::new(()),
        }
    }

    #[frb(sync)]
    pub fn push_line(&mut self, len_chars: usize, height: f32, is_folded: bool) {
        self.tree.push(LineBlock { len_chars, height, is_folded }, ());
    }

    #[frb(sync)]
    pub fn clear(&mut self) {
        self.tree = SumTree::new(());
    }
}

pub struct RustFoldRange {
    pub start_line: i32,
    pub end_line: i32,
}

pub fn folds_compute_all(rope: &RopeBridge) -> Vec<RustFoldRange> {
    let mut folds = Vec::new();
    let mut stack: Vec<(char, i32)> = Vec::new();
    let mut line_idx: i32 = 0;

    let rope = rope.rope.read().unwrap();
    for ch in rope.chars() {
        if ch == '\n' {
            line_idx += 1;
        }
        if ch == '{' || ch == '[' || ch == '(' {
            stack.push((ch, line_idx));
        } else if ch == '}' || ch == ']' || ch == ')' {
            if let Some((open_ch, start_line)) = stack.pop() {
                let matches = match (open_ch, ch) {
                    ('{', '}') => true,
                    ('[', ']') => true,
                    ('(', ')') => true,
                    _ => false,
                };
                if matches && start_line < line_idx {
                    folds.push(RustFoldRange {
                        start_line,
                        end_line: line_idx,
                    });
                }
            }
        }
    }
    folds
}

#[frb(sync)]
pub fn folds_find_matching_bracket(rope: &RopeBridge, target_offset: i32) -> i64 {
    if target_offset < 0 {
        return -1;
    }
    let rope = rope.rope.read().unwrap();
    find_matching_bracket_in_rope(&rope, target_offset as usize)
        .map(|idx| idx as i64)
        .unwrap_or(-1)
}

#[derive(Clone, Debug)]
pub struct GuideBlock {
    pub start_line: i32,
    pub end_line: i32,
    pub indent_level: i32,
    pub leading_spaces: i32,
}

#[frb(sync)]
pub fn guides_compute_viewport(
    _rope: &RopeBridge,
    _first_visible: usize,
    _last_visible: usize,
    _tab_size: usize,
) -> Vec<GuideBlock> {
    let scan_back_limit: usize = 500;

    let rope_lock = _rope.rope.read().unwrap();
    let total_lines = rope_lock.len_lines();

    if total_lines == 0 {
        return Vec::new();
    }

    let first = _first_visible.min(total_lines.saturating_sub(1));
    let last = _last_visible.min(total_lines.saturating_sub(1));
    let scan_start = if first > scan_back_limit { first - scan_back_limit } else { 0 };

    let mut blocks: Vec<GuideBlock> = Vec::new();

    for line_idx in scan_start..=last {
        let line_slice = rope_lock.line(line_idx);
        let mut line_str = line_slice.to_string();
        if line_str.ends_with("\r\n") {
            line_str.truncate(line_str.len() - 2);
        } else if line_str.ends_with('\n') {
            line_str.truncate(line_str.len() - 1);
        }

        let trimmed_right = line_str.trim_end();
        if trimmed_right.is_empty() {
            continue;
        }

        let last_char = trimmed_right.chars().last().unwrap();
        let opening_tag_name = extract_opening_tag_name(trimmed_right);
        let ends_with_bracket = matches!(last_char, '{' | '(' | '[' | ':');
        if !ends_with_bracket && opening_tag_name.is_none() {
            continue;
        }

        let mut leading_cols: usize = 0;
        for c in trimmed_right.chars() {
            if c == ' ' {
                leading_cols += 1;
            } else if c == '\t' {
                leading_cols += _tab_size;
            } else {
                break;
            }
        }

        let indent_level = if _tab_size > 0 { leading_cols / _tab_size } else { 0 };

        let mut end_line: usize = line_idx + 1;

        if matches!(last_char, '{' | '(' | '[') {
            let line_start_char = rope_lock.line_to_char(line_idx);
            let trimmed_len = trimmed_right.chars().count();
            if trimmed_len > 0 {
                let bracket_pos = line_start_char + trimmed_len - 1;
                if let Some(match_pos) = find_matching_bracket_in_rope(&rope_lock, bracket_pos)
                {
                    let matched_line = rope_lock.char_to_line(match_pos);
                    end_line = matched_line + 1;
                }
            }
        } else if let Some(tag_name) = opening_tag_name {
            if let Some(match_line) = find_matching_closing_tag_line(&rope_lock, line_idx, &tag_name) {
                end_line = match_line + 1;
            }
        }

        if end_line <= line_idx + 1 {
            let mut scan = line_idx + 1;
            let mut last_valid = line_idx;
            while scan < total_lines {
                let mut nl = rope_lock.line(scan).to_string();
                if nl.ends_with("\r\n") {
                    nl.truncate(nl.len() - 2);
                } else if nl.ends_with('\n') {
                    nl.truncate(nl.len() - 1);
                }
                if nl.trim().is_empty() {
                    scan += 1;
                    continue;
                }
                let mut next_leading: usize = 0;
                for c in nl.chars() {
                    if c == ' ' {
                        next_leading += 1;
                    } else if c == '\t' {
                        next_leading += _tab_size;
                    } else {
                        break;
                    }
                }
                if next_leading <= leading_cols {
                    break;
                }
                last_valid = scan;
                scan += 1;
            }
            end_line = last_valid + 1;
        }

        if end_line <= line_idx + 1 {
            continue;
        }

        let mut would_pass = false;
        if end_line > line_idx + 1 {
            for check in (line_idx + 1)..(end_line.saturating_sub(1)) {
                let mut cl = rope_lock.line(check).to_string();
                if cl.ends_with("\r\n") {
                    cl.truncate(cl.len() - 2);
                } else if cl.ends_with('\n') {
                    cl.truncate(cl.len() - 1);
                }
                if cl.trim().is_empty() {
                    continue;
                }
                let mut check_leading: usize = 0;
                for c in cl.chars() {
                    if c == ' ' {
                        check_leading += 1;
                    } else if c == '\t' {
                        check_leading += _tab_size;
                    } else {
                        break;
                    }
                }
                if check_leading <= leading_cols {
                    would_pass = true;
                    break;
                }
            }
        }

        if would_pass {
            continue;
        }

        blocks.push(GuideBlock {
            start_line: line_idx as i32,
            end_line: end_line as i32,
            indent_level: indent_level as i32,
            leading_spaces: leading_cols as i32,
        });
    }

    blocks
}

fn find_matching_bracket_in_rope(rope: &RustRope, target_offset: usize) -> Option<usize> {
    let len = rope.len_chars();
    if target_offset >= len {
        return None;
    }

    let start_ch: char = rope.char(target_offset);

    let (matcher, search_forward) = match start_ch {
        '{' => ('}', true),
        '[' => (']', true),
        '(' => (')', true),
        '}' => ('{', false),
        ']' => ('[', false),
        ')' => ('(', false),
        _ => return None,
    };

    let mut depth = 1;

    if search_forward {
        for (i, ch) in rope.chars_at(target_offset + 1).enumerate() {
            let idx = target_offset + 1 + i;
            if idx >= len {
                break;
            }
            if ch == start_ch {
                depth += 1;
            } else if ch == matcher {
                depth -= 1;
                if depth == 0 {
                    return Some(idx);
                }
            }
        }
    } else {
        if target_offset == 0 {
            return None;
        }
        let mut idx = target_offset;
        while idx > 0 {
            let (chunk, chunk_byte_idx, chunk_char_idx, _) = rope.chunk_at_char(idx - 1);
            let chunk_start_char = chunk_char_idx;
            let chunk_start_byte = chunk_byte_idx;
            let global_byte = rope.char_to_byte(idx);
            let local_byte = global_byte.saturating_sub(chunk_start_byte);
            let prefix = &chunk[..local_byte];

            for ch in prefix.chars().rev() {
                idx -= 1;
                if ch == start_ch {
                    depth += 1;
                } else if ch == matcher {
                    depth -= 1;
                    if depth == 0 {
                        return Some(idx);
                    }
                }

                if idx == chunk_start_char {
                    break;
                }
            }
        }
    }

    None
}

fn extract_opening_tag_name(line: &str) -> Option<String> {
    let trimmed = line.trim_end();
    if !trimmed.ends_with('>') || trimmed.ends_with("/>") || trimmed.ends_with("-->") {
        return None;
    }

    let start = trimmed.find('<')?;
    let mut chars = trimmed[start + 1..].chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }

    let mut name = String::new();
    name.push(first);
    for ch in chars {
        if ch.is_ascii_alphanumeric() || matches!(ch, ':' | '_' | '-') {
            name.push(ch);
        } else {
            break;
        }
    }

    if name.is_empty() { None } else { Some(name) }
}

fn find_matching_closing_tag_line(
    rope: &RustRope,
    start_line: usize,
    tag_name: &str,
) -> Option<usize> {
    let mut depth = 1usize;
    let total_lines = rope.len_lines();

    for line_idx in (start_line + 1)..total_lines {
        let mut line_str = rope.line(line_idx).to_string();
        if line_str.ends_with("\r\n") {
            line_str.truncate(line_str.len() - 2);
        } else if line_str.ends_with('\n') {
            line_str.truncate(line_str.len() - 1);
        }

        let trimmed = line_str.trim();
        if trimmed.is_empty() {
            continue;
        }

        if contains_same_tag_opening(trimmed, tag_name) {
            depth += 1;
        }

        if contains_same_tag_closing(trimmed, tag_name) {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(line_idx);
            }
        }
    }

    None
}

fn contains_same_tag_opening(line: &str, tag_name: &str) -> bool {
    let opening = format!("<{}", tag_name);
    line.contains(&opening) && !line.contains(&format!("</{}", tag_name)) && !line.ends_with("/>")
}

fn contains_same_tag_closing(line: &str, tag_name: &str) -> bool {
    line.contains(&format!("</{}", tag_name))
}

#[frb(sync)]
pub fn words_extract(rope: &RopeBridge) -> Vec<String> {
    let mut words = HashSet::new();
    let mut current_word = String::new();

    let rope = rope.rope.read().unwrap();
    for ch in rope.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            current_word.push(ch);
        } else {
            if !current_word.is_empty() {
                words.insert(mem::take(&mut current_word));
            }
        }
    }
    if !current_word.is_empty() {
        words.insert(current_word);
    }
    
    words.into_iter().collect()
}
