// Workaround for Claude Code's React Ink placing the terminal cursor
// at the wrong position during IME composition.
// Remove this entire module once Claude Code fixes cursor positioning.

use growterm_types::{Cell, CellFlags};

const CLAUDE_PROCESS_NAME: &str = "claude";

pub struct InkImeState {
    ink_app_cached: Option<bool>,
    trailing_spaces: u16,
}

impl InkImeState {
    pub fn new() -> Self {
        Self {
            ink_app_cached: None,
            trailing_spaces: 0,
        }
    }

    /// Track trailing spaces from committed text.
    pub fn on_text_commit(&mut self, text: &str) {
        if self.is_active() {
            if text.chars().all(|c| c == ' ') {
                self.trailing_spaces = self
                    .trailing_spaces
                    .saturating_add(text.chars().count() as u16);
            } else {
                self.trailing_spaces =
                    text.chars().rev().take_while(|&c| c == ' ').count() as u16;
            }
        }
    }

    /// Adjust trailing space count on backspace.
    pub fn on_key_input(&mut self, bytes: &[u8]) {
        if self.is_active() && self.trailing_spaces > 0 {
            if bytes == b"\x7f" || bytes == b"\x08" {
                self.trailing_spaces = self.trailing_spaces.saturating_sub(1);
            } else if bytes != b"\r" && bytes != b"\n" {
                // Any other non-enter key input resets trailing spaces
                // (arrow keys, etc. mean cursor moved away from end)
            }
        }
    }

    /// Reset trailing spaces on Enter.
    pub fn on_enter(&mut self) {
        self.trailing_spaces = 0;
    }

    /// Detect whether the active PTY is running a Claude Code process.
    pub fn on_preedit(&mut self, child_pid: Option<u32>) {
        self.on_preedit_with(child_pid, has_descendant_named);
    }

    fn on_preedit_with(
        &mut self,
        child_pid: Option<u32>,
        checker: impl FnOnce(u32, &str) -> bool,
    ) {
        if let Some(pid) = child_pid {
            let found = checker(pid, CLAUDE_PROCESS_NAME);
            self.ink_app_cached = Some(found);
            if !found {
                self.trailing_spaces = 0;
            }
        }
    }

    pub fn is_active(&self) -> bool {
        self.ink_app_cached == Some(true)
    }

    /// Calculate overridden preedit position for Ink apps.
    ///
    /// Ink renders its cursor as an INVERSE cell. We find that cell in the
    /// input area and use its position. Falls back to cell content scan
    /// with trailing space tracking when no INVERSE cell is found.
    pub fn preedit_pos(&self, cells: &[Vec<Cell>]) -> Option<(u16, u16)> {
        if !self.is_active() {
            return None;
        }
        let prompt_row = find_prompt_row(cells)?;
        let bottom = find_input_bottom(cells, prompt_row);

        // Primary: find Ink's INVERSE cursor cell
        if let Some(pos) = find_ink_cursor(cells, prompt_row, bottom) {
            return Some(pos);
        }

        // Fallback: content scan + trailing spaces
        let (row, col) = find_input_end(cells, prompt_row, bottom);
        Some((row, col + self.trailing_spaces))
    }
}

/// Find Ink's cursor (INVERSE cell) in the input area.
fn find_ink_cursor(cells: &[Vec<Cell>], prompt_row: usize, bottom: usize) -> Option<(u16, u16)> {
    for row_idx in (prompt_row..=bottom).rev() {
        for (col, cell) in cells[row_idx].iter().enumerate() {
            if cell.flags.contains(CellFlags::INVERSE) {
                return Some((row_idx as u16, col as u16));
            }
        }
    }
    None
}

/// Find the row index of the last input row (before the next separator).
pub fn find_input_bottom(cells: &[Vec<Cell>], prompt_row: usize) -> usize {
    for row_idx in (prompt_row + 1)..cells.len() {
        if cells[row_idx]
            .first()
            .map_or(false, |c| c.character == '─')
        {
            return row_idx - 1;
        }
    }
    cells.len().saturating_sub(1)
}

/// Scan from prompt_row to bottom to find the position just after the last
/// non-blank cell.
fn find_input_end(cells: &[Vec<Cell>], prompt_row: usize, bottom: usize) -> (u16, u16) {
    let mut last_row = prompt_row;
    let mut last_col_end: usize = 0;

    for row_idx in prompt_row..=bottom {
        let row = &cells[row_idx];
        for (col, cell) in row.iter().enumerate() {
            if cell.character != ' ' && cell.character != '\0' {
                last_row = row_idx;
                if cell.flags.contains(CellFlags::WIDE_CHAR) {
                    last_col_end = col + 2;
                } else {
                    last_col_end = col + 1;
                }
            }
        }
    }

    (last_row as u16, last_col_end as u16)
}

/// Walk the process tree to check if any descendant has the given name.
fn has_descendant_named(root_pid: u32, name: &str) -> bool {
    let output = match std::process::Command::new("ps")
        .args(["-eo", "pid,ppid,comm="])
        .output()
    {
        Ok(o) => o,
        Err(_) => return false,
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut children: std::collections::HashMap<u32, Vec<(u32, String)>> =
        std::collections::HashMap::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }
        let pid: u32 = match parts[0].parse() {
            Ok(p) => p,
            Err(_) => continue,
        };
        let ppid: u32 = match parts[1].parse() {
            Ok(p) => p,
            Err(_) => continue,
        };
        let comm = parts[2..].join(" ");
        children.entry(ppid).or_default().push((pid, comm));
    }
    let mut stack = vec![root_pid];
    while let Some(pid) = stack.pop() {
        if let Some(kids) = children.get(&pid) {
            for (kid_pid, comm) in kids {
                if comm.contains(name) {
                    return true;
                }
                stack.push(*kid_pid);
            }
        }
    }
    false
}

/// Find the prompt row (❯) between two separator lines (─) in the grid.
pub fn find_prompt_row(cells: &[Vec<Cell>]) -> Option<usize> {
    let is_separator = |row: &[Cell]| -> bool {
        row.first().map_or(false, |c| c.character == '─')
    };
    let separators: Vec<usize> = cells
        .iter()
        .enumerate()
        .filter(|(_, row)| is_separator(row))
        .map(|(i, _)| i)
        .collect();
    // Check after the last separator first (new prompt may lack bottom separator)
    if let Some(&last_sep) = separators.last() {
        for row_idx in (last_sep + 1)..cells.len() {
            if cells[row_idx].iter().any(|c| c.character == '❯') {
                return Some(row_idx);
            }
        }
    }
    // Fall back to between separator pairs (from bottom)
    for window in separators.windows(2).rev() {
        let (top, bottom) = (window[0], window[1]);
        for row_idx in (top + 1)..bottom {
            if cells[row_idx].iter().any(|c| c.character == '❯') {
                return Some(row_idx);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use growterm_types::Cell;

    fn make_row(chars: &str, width: usize) -> Vec<Cell> {
        let mut row = vec![Cell::default(); width];
        for (i, ch) in chars.chars().enumerate() {
            if i < width {
                row[i].character = ch;
            }
        }
        row
    }

    fn make_row_with_wide(segments: &[(&str, bool)], width: usize) -> Vec<Cell> {
        let mut row = vec![Cell::default(); width];
        let mut col = 0;
        for &(text, wide) in segments {
            for ch in text.chars() {
                if col >= width {
                    break;
                }
                row[col].character = ch;
                if wide {
                    row[col].flags = CellFlags::WIDE_CHAR;
                    col += 2;
                } else {
                    col += 1;
                }
            }
        }
        row
    }

    fn active_state() -> InkImeState {
        InkImeState {
            ink_app_cached: Some(true),
            trailing_spaces: 0,
        }
    }

    // --- find_prompt_row ---

    #[test]
    fn find_prompt_row_between_separators() {
        let cells = vec![
            make_row("hello", 80),
            make_row("─────", 80),
            make_row("❯ ", 80),
            make_row("─────", 80),
            make_row("output", 80),
        ];
        assert_eq!(find_prompt_row(&cells), Some(2));
    }

    #[test]
    fn find_prompt_row_no_separators() {
        let cells = vec![make_row("hello", 80), make_row("❯ ", 80)];
        assert_eq!(find_prompt_row(&cells), None);
    }

    #[test]
    fn find_prompt_row_after_last_separator() {
        // New prompt after Enter — bottom separator not yet drawn
        let cells = vec![
            make_row("─────", 80),
            make_row("❯ old input", 80),
            make_row("─────", 80),
            make_row("output", 80),
            make_row("─────", 80),
            make_row("❯ ", 80),
        ];
        assert_eq!(find_prompt_row(&cells), Some(5));
    }

    #[test]
    fn find_prompt_row_prefers_after_last_separator() {
        // Both old prompt (between separators) and new prompt (after last separator)
        let cells = vec![
            make_row("─────", 80),
            make_row("❯ old", 80),
            make_row("─────", 80),
            make_row("❯ new", 80),
        ];
        // Should prefer row 3 (after last separator)
        assert_eq!(find_prompt_row(&cells), Some(3));
    }

    #[test]
    fn find_prompt_row_no_prompt() {
        let cells = vec![
            make_row("─────", 80),
            make_row("hello", 80),
            make_row("─────", 80),
        ];
        assert_eq!(find_prompt_row(&cells), None);
    }

    fn set_inverse(row: &mut Vec<Cell>, col: usize) {
        row[col].flags |= CellFlags::INVERSE;
    }

    // --- preedit_pos ---

    #[test]
    fn preedit_pos_uses_inverse_cursor() {
        let state = active_state();
        let mut row1 = make_row_with_wide(&[("❯ ", false), ("하이", true)], 80);
        let mut row2 = make_row("  ", 80);
        set_inverse(&mut row2, 2); // Ink cursor at col 2 on empty line
        let cells = vec![
            make_row("─────", 80),
            row1,
            row2,
            make_row("─────", 80),
        ];
        // Should use INVERSE cell position, not content scan
        assert_eq!(state.preedit_pos(&cells), Some((2, 2)));
    }

    #[test]
    fn preedit_pos_inverse_cursor_after_content() {
        let state = active_state();
        let mut row1 = make_row_with_wide(&[("❯ ", false), ("하이", true)], 80);
        set_inverse(&mut row1, 6); // Ink cursor right after content
        let cells = vec![
            make_row("─────", 80),
            row1,
            make_row("─────", 80),
        ];
        assert_eq!(state.preedit_pos(&cells), Some((1, 6)));
    }

    #[test]
    fn preedit_pos_not_active() {
        let state = InkImeState::new();
        let cells = vec![
            make_row("─────", 80),
            make_row("❯ ", 80),
            make_row("─────", 80),
        ];
        assert_eq!(state.preedit_pos(&cells), None);
    }

    #[test]
    fn preedit_pos_english_text() {
        let state = active_state();
        let cells = vec![
            make_row("─────", 80),
            make_row("❯ hello", 80),
            make_row("─────", 80),
        ];
        assert_eq!(state.preedit_pos(&cells), Some((1, 7)));
    }

    #[test]
    fn preedit_pos_korean_wide_chars() {
        let state = active_state();
        let cells = vec![
            make_row("─────", 80),
            make_row_with_wide(&[("❯ ", false), ("한글", true)], 80),
            make_row("─────", 80),
        ];
        assert_eq!(state.preedit_pos(&cells), Some((1, 6)));
    }

    #[test]
    fn preedit_pos_with_trailing_space() {
        let mut state = active_state();
        state.on_text_commit("hello");
        state.on_text_commit(" ");
        let cells = vec![
            make_row("─────", 80),
            make_row("❯ hello ", 80),
            make_row("─────", 80),
        ];
        // scan finds 'o' at col 6 → end=7, trailing_spaces=1 → 8
        assert_eq!(state.preedit_pos(&cells), Some((1, 8)));
    }

    #[test]
    fn preedit_pos_trailing_space_after_korean() {
        let mut state = active_state();
        state.on_text_commit("안녕");
        state.on_text_commit(" ");
        let cells = vec![
            make_row("─────", 80),
            make_row_with_wide(&[("❯ ", false), ("안녕", true)], 80),
            make_row("─────", 80),
        ];
        // scan finds '녕' at col 4, WIDE → end=6, trailing_spaces=1 → 7
        assert_eq!(state.preedit_pos(&cells), Some((1, 7)));
    }

    #[test]
    fn preedit_pos_backspace_removes_trailing_space() {
        let mut state = active_state();
        state.on_text_commit("hello ");
        assert_eq!(state.trailing_spaces, 1);
        state.on_key_input(b"\x7f"); // backspace
        assert_eq!(state.trailing_spaces, 0);
        let cells = vec![
            make_row("─────", 80),
            make_row("❯ hello", 80),
            make_row("─────", 80),
        ];
        assert_eq!(state.preedit_pos(&cells), Some((1, 7)));
    }

    #[test]
    fn on_text_commit_resets_on_non_space() {
        let mut state = active_state();
        state.on_text_commit(" ");
        assert_eq!(state.trailing_spaces, 1);
        state.on_text_commit("a");
        assert_eq!(state.trailing_spaces, 0);
    }

    #[test]
    fn on_text_commit_accumulates_spaces() {
        let mut state = active_state();
        state.on_text_commit(" ");
        state.on_text_commit(" ");
        assert_eq!(state.trailing_spaces, 2);
    }

    #[test]
    fn on_text_commit_trailing_spaces_in_mixed() {
        let mut state = active_state();
        state.on_text_commit("hello  ");
        assert_eq!(state.trailing_spaces, 2);
    }

    #[test]
    fn on_enter_resets_trailing_spaces() {
        let mut state = active_state();
        state.on_text_commit(" ");
        state.on_enter();
        assert_eq!(state.trailing_spaces, 0);
    }

    #[test]
    fn preedit_pos_multiline_input() {
        let state = active_state();
        let cells = vec![
            make_row("─────", 80),
            make_row("❯ first line", 80),
            make_row("  second", 80),
            make_row("─────", 80),
        ];
        assert_eq!(state.preedit_pos(&cells), Some((2, 8)));
    }

    #[test]
    fn preedit_pos_empty_prompt() {
        let state = active_state();
        let cells = vec![
            make_row("─────", 80),
            make_row("❯ ", 80),
            make_row("─────", 80),
        ];
        assert_eq!(state.preedit_pos(&cells), Some((1, 1)));
    }

    // --- on_preedit ---

    #[test]
    fn on_preedit_detects_claude() {
        let mut state = InkImeState::new();
        state.on_preedit_with(Some(1), |_, _| true);
        assert!(state.is_active());
    }

    #[test]
    fn on_preedit_not_claude() {
        let mut state = active_state();
        state.trailing_spaces = 5;
        state.on_preedit_with(Some(1), |_, _| false);
        assert!(!state.is_active());
        assert_eq!(state.trailing_spaces, 0);
    }

    #[test]
    fn on_preedit_noop_without_pid() {
        let mut state = InkImeState::new();
        state.on_preedit_with(None, |_, _| panic!("should not be called"));
        assert_eq!(state.ink_app_cached, None);
    }
}
