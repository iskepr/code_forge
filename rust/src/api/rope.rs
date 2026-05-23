use ropey::Rope as RustRope;
use std::sync::RwLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextDirection {
    Ltr,
    Rtl,
    Mixed,
}

#[derive(Clone, Copy, Debug)]
pub struct BiDiSegment {
    pub start: usize,
    pub end: usize,
    pub direction: TextDirection,
}

#[derive(Clone, Copy, Debug)]
pub struct SelectionState {
    pub base_offset: usize,
    pub extent_offset: usize,
}

#[flutter_rust_bridge::frb(opaque)]
pub struct RopeBridge {
    pub(crate) rope: RwLock<RustRope>,
    selection: RwLock<SelectionState>,
}

impl RopeBridge {
    #[flutter_rust_bridge::frb(sync)]
    pub fn create(initial_text: String) -> Self {
        Self {
            rope: RwLock::new(RustRope::from_str(&initial_text)),
            selection: RwLock::new(SelectionState {
                base_offset: 0,
                extent_offset: 0,
            }),
        }
    }

    #[flutter_rust_bridge::frb(sync)]
    pub fn selection(&self) -> SelectionState {
        *self.selection.read().unwrap()
    }

    #[flutter_rust_bridge::frb(sync)]
    pub fn set_selection(&self, base_offset: usize, extent_offset: usize) {
        let len = self.rope.read().unwrap().len_chars();
        let clamped_base = base_offset.min(len);
        let clamped_extent = extent_offset.min(len);
        *self.selection.write().unwrap() = SelectionState {
            base_offset: clamped_base,
            extent_offset: clamped_extent,
        };
    }

    #[flutter_rust_bridge::frb(sync)]
    pub fn replace_range_and_update_selection(
        &self,
        start: usize,
        end: usize,
        replacement: String,
        preserve_old_cursor: bool,
        old_base: usize,
        old_extent: usize,
    ) -> SelectionState {
        let mut rope_write = self.rope.write().unwrap();
        let len = rope_write.len_chars();
        let safe_start = start.min(len);
        let safe_end = end.clamp(safe_start, len);

        if safe_start < safe_end {
            rope_write.remove(safe_start..safe_end);
        }
        if !replacement.is_empty() {
            rope_write.insert(safe_start, &replacement);
        }

        let new_selection = if preserve_old_cursor {
            let delta = replacement.len() as isize - (safe_end - safe_start) as isize;

            let map_offset = |offset: usize| -> usize {
                if offset <= safe_start {
                    offset
                } else if offset >= safe_end {
                    let mapped = (offset as isize + delta) as isize;
                    mapped.clamp(0, rope_write.len_chars() as isize) as usize
                } else {
                    let relative = offset.saturating_sub(safe_start);
                    let mapped = safe_start + relative.min(replacement.len());
                    mapped.min(rope_write.len_chars())
                }
            };

            let base = map_offset(old_base);
            let extent = map_offset(old_extent);
            SelectionState {
                base_offset: base,
                extent_offset: extent,
            }
        } else {
            SelectionState {
                base_offset: safe_start + replacement.len(),
                extent_offset: safe_start + replacement.len(),
            }
        };

        // update stored selection
        *self.selection.write().unwrap() = SelectionState {
            base_offset: new_selection.base_offset.min(rope_write.len_chars()),
            extent_offset: new_selection.extent_offset.min(rope_write.len_chars()),
        };

        *self.selection.read().unwrap()
    }

    #[flutter_rust_bridge::frb(sync)]
    pub fn len_chars(&self) -> usize {
        self.rope.read().unwrap().len_chars()
    }

    #[flutter_rust_bridge::frb(sync)]
    pub fn get_text(&self) -> String {
        self.rope.read().unwrap().to_string()
    }

    #[flutter_rust_bridge::frb(sync)]
    pub fn insert(&self, char_idx: usize, text: String) {
        self.rope.write().unwrap().insert(char_idx, &text);
    }

    #[flutter_rust_bridge::frb(sync)]
    pub fn remove(&self, start: usize, end: usize) {
        self.rope.write().unwrap().remove(start..end);
    }

    #[flutter_rust_bridge::frb(sync)]
    pub fn slice(&self, start: usize, end: usize) -> String {
        let rope = self.rope.read().unwrap();
        let valid_start = start.min(rope.len_chars());
        let valid_end = end.max(valid_start).min(rope.len_chars());
        rope.slice(valid_start..valid_end).to_string()
    }

    #[flutter_rust_bridge::frb(sync)]
    pub fn char_to_line(&self, char_idx: usize) -> usize {
        let rope = self.rope.read().unwrap();
        let valid_idx = char_idx.min(rope.len_chars());
        rope.char_to_line(valid_idx)
    }

    #[flutter_rust_bridge::frb(sync)]
    pub fn line_to_char(&self, line_idx: usize) -> usize {
        let rope = self.rope.read().unwrap();
        let valid_idx = line_idx.min(rope.len_lines().saturating_sub(1));
        rope.line_to_char(valid_idx)
    }

    #[flutter_rust_bridge::frb(sync)]
    pub fn line(&self, line_idx: usize) -> String {
        let rope = self.rope.read().unwrap();
        let valid_idx = line_idx.min(rope.len_lines().saturating_sub(1));
        let mut line_str = rope.line(valid_idx).to_string();
        if line_str.ends_with("\r\n") {
            line_str.truncate(line_str.len() - 2);
        } else if line_str.ends_with('\n') {
            line_str.truncate(line_str.len() - 1);
        }
        line_str
    }
    
    #[flutter_rust_bridge::frb(sync)]
    pub fn len_lines(&self) -> usize {
        self.rope.read().unwrap().len_lines()
    }

    #[flutter_rust_bridge::frb(sync)]
    pub fn char_at(&self, position: usize) -> String {
        let rope = self.rope.read().unwrap();
        if position >= rope.len_chars() {
            return String::new();
        }
        rope.char(position).to_string()
    }
    
    #[flutter_rust_bridge::frb(sync)]
    pub fn copy(&self) -> Self {
        Self {
            rope: RwLock::new(self.rope.read().unwrap().clone()),
            selection: RwLock::new(*self.selection.read().unwrap()),
        }
    }

    #[flutter_rust_bridge::frb(sync)]
    pub fn cached_lines(&self) -> Vec<String> {
        let rope = self.rope.read().unwrap();
        let mut lines = Vec::with_capacity(rope.len_lines());
        for line in rope.lines() {
            let mut line_str = line.to_string();
            if line_str.ends_with("\r\n") {
                line_str.truncate(line_str.len() - 2);
            } else if line_str.ends_with('\n') {
                line_str.truncate(line_str.len() - 1);
            }
            lines.push(line_str);
        }
        lines
    }

    #[flutter_rust_bridge::frb(sync)]
    pub fn cached_lines_range(&self, start_line: usize, end_line: usize) -> Vec<String> {
        let rope = self.rope.read().unwrap();
        let total = rope.len_lines();
        let start = start_line.min(total);
        let end = end_line.min(total).max(start);
        let mut lines = Vec::with_capacity(end.saturating_sub(start));
        for line_idx in start..end {
            let mut line_str = rope.line(line_idx).to_string();
            if line_str.ends_with("\r\n") {
                line_str.truncate(line_str.len() - 2);
            } else if line_str.ends_with('\n') {
                line_str.truncate(line_str.len() - 1);
            }
            lines.push(line_str);
        }
        lines
    }
    
    #[flutter_rust_bridge::frb(sync)]
    pub fn primary_direction(&self) -> TextDirection {
        let rope = self.rope.read().unwrap();
        let mut rtl_count = 0;
        let mut ltr_count = 0;
        
        for c in rope.chars() {
            if is_rtl_char(c) {
                rtl_count += 1;
            } else if is_ltr_char(c) {
                ltr_count += 1;
            }
        }
        
        if rtl_count == 0 && ltr_count == 0 {
            TextDirection::Ltr
        } else if rtl_count > ltr_count {
            TextDirection::Rtl
        } else {
            TextDirection::Ltr
        }
    }

    #[flutter_rust_bridge::frb(sync)]
    pub fn text_direction(&self) -> TextDirection {
        let rope: std::sync::RwLockReadGuard<'_, RustRope> = self.rope.read().unwrap();
        let mut has_rtl = false;
        let mut has_ltr = false;
        
        for c in rope.chars() {
            if is_rtl_char(c) {
                has_rtl = true;
            } else if is_ltr_char(c) {
                has_ltr = true;
            }
            if has_rtl && has_ltr {
                return TextDirection::Mixed;
            }
        }
        
        if !has_rtl && !has_ltr { TextDirection::Ltr }
        else if !has_rtl { TextDirection::Ltr }
        else { TextDirection::Rtl }
    }

    #[flutter_rust_bridge::frb(sync)]
    pub fn get_bidi_segments_in_range(&self, start: usize, end: usize) -> Vec<BiDiSegment> {
        let rope: std::sync::RwLockReadGuard<'_, RustRope> = self.rope.read().unwrap();
        compute_bidi_segments(&rope, start, end)
    }
    
    #[flutter_rust_bridge::frb(sync)]
    pub fn get_bidi_segments_for_line(&self, line_index: usize) -> Vec<BiDiSegment> {
        let rope: std::sync::RwLockReadGuard<'_, RustRope> = self.rope.read().unwrap();
        let valid_idx: usize = line_index.min(rope.len_lines().saturating_sub(1));
        let start: usize = rope.line_to_char(valid_idx);
        let line: ropey::RopeSlice<'_> = rope.line(valid_idx);
        let end: usize = start + line.len_chars();
        compute_bidi_segments(&rope, start, end)
    }

    #[flutter_rust_bridge::frb(sync)]
    pub fn find_line_start(&self, offset: usize) -> usize {
        let line = self.char_to_line(offset);
        self.line_to_char(line)
    }

    #[flutter_rust_bridge::frb(sync)]
    pub fn find_line_end(&self, offset: usize) -> usize {
        let rope = self.rope.read().unwrap();
        let valid_offset = offset.min(rope.len_chars());
        let line = rope.char_to_line(valid_offset);
        let next_line_start = if line + 1 < rope.len_lines() {
            rope.line_to_char(line + 1)
        } else {
            rope.len_chars()
        };
        let line_slice = rope.line(line);
        let line_len = line_slice.len_chars();
        if line_len == 0 {
            return next_line_start;
        }
        let last = line_slice.char(line_len - 1);
        if last == '\n' {
            if line_len >= 2 && line_slice.char(line_len - 2) == '\r' {
                next_line_start.saturating_sub(2)
            } else {
                next_line_start.saturating_sub(1)
            }
        } else {
            next_line_start
        }
    }
}

fn compute_bidi_segments(rope: &RustRope, start: usize, end: usize) -> Vec<BiDiSegment> {
    let end: usize = end.min(rope.len_chars());
    if start >= end {
        return vec![];
    }

    let mut segments = Vec::new();
    let mut current_dir = None;
    let mut segment_start = 0;

    let slice = rope.slice(start..end);

    for (i, c) in slice.chars().enumerate() {
        let char_dir = if is_rtl_char(c) {
            Some(TextDirection::Rtl)
        } else if is_ltr_char(c) {
            Some(TextDirection::Ltr)
        } else {
            None
        };

        if let Some(cd) = char_dir {
            if current_dir.is_none() {
                current_dir = Some(cd);
                segment_start = i;
            } else if Some(cd) != current_dir {
                segments.push(BiDiSegment {
                    start: start + segment_start,
                    end: start + i,
                    direction: current_dir.unwrap(),
                });
                current_dir = Some(cd);
                segment_start = i;
            }
        }
    }

    if let Some(cd) = current_dir {
        segments.push(BiDiSegment {
            start: start + segment_start,
            end,
            direction: cd,
        });
    }

    segments
}

fn is_rtl_char(c: char) -> bool {
    let code = c as u32;
    (code >= 0x0600 && code <= 0x06FF) ||
    (code >= 0x0750 && code <= 0x077F) ||
    (code >= 0x08A0 && code <= 0x08FF) ||
    (code >= 0x0590 && code <= 0x05FF) ||
    (code >= 0x0700 && code <= 0x074F) ||
    (code >= 0x0780 && code <= 0x07BF) ||
    (code >= 0x07C0 && code <= 0x07FF) ||
    matches!(code, 0x200F | 0x202B | 0x202E | 0x2067)
}

fn is_ltr_char(c: char) -> bool {
    let code = c as u32;
    (code >= 0x0041 && code <= 0x005A) ||
    (code >= 0x0061 && code <= 0x007A) ||
    (code >= 0x00C0 && code <= 0x00FF) ||
    (code >= 0x0100 && code <= 0x017F) ||
    (code >= 0x0180 && code <= 0x024F) ||
    (code >= 0x0370 && code <= 0x03FF) ||
    (code >= 0x0400 && code <= 0x04FF) ||
    matches!(code, 0x200E | 0x202A | 0x202D | 0x2066)
}
