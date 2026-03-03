use growterm_types::{Cell, CellFlags};

#[derive(Debug, Clone, Copy, Default)]
pub struct Selection {
    /// Absolute row (scrollback + screen), column
    pub start: (u32, u16),
    pub end: (u32, u16),
    pub active: bool,
}

impl Selection {
    pub fn begin(&mut self, row: u32, col: u16) {
        self.start = (row, col);
        self.end = (row, col);
        self.active = true;
    }

    pub fn update(&mut self, row: u32, col: u16) {
        self.end = (row, col);
    }

    pub fn finish(&mut self) {
        self.active = false;
    }

    pub fn clear(&mut self) {
        self.active = false;
        self.start = (0, 0);
        self.end = (0, 0);
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Returns (start, end) in normalized order (top-left to bottom-right)
    pub fn normalized(&self) -> ((u32, u16), (u32, u16)) {
        let (s, e) = (self.start, self.end);
        if s.0 < e.0 || (s.0 == e.0 && s.1 <= e.1) {
            (s, e)
        } else {
            (e, s)
        }
    }

    /// Convert absolute selection to screen-relative for rendering.
    /// Returns None if the selection is entirely off-screen.
    pub fn screen_normalized(&self, view_base: u32, visible_rows: u16) -> Option<((u16, u16), (u16, u16))> {
        if self.is_empty() {
            return None;
        }
        let ((sr, sc), (er, ec)) = self.normalized();
        let view_end = view_base + visible_rows as u32;
        // Entirely off-screen?
        if er < view_base || sr >= view_end {
            return None;
        }
        let screen_sr = if sr >= view_base { (sr - view_base) as u16 } else { 0 };
        let screen_sc = if sr >= view_base { sc } else { 0 };
        let screen_er = if er < view_end { (er - view_base) as u16 } else { visible_rows - 1 };
        let screen_ec = if er < view_end { ec } else { u16::MAX };
        Some(((screen_sr, screen_sc), (screen_er, screen_ec)))
    }

    pub fn contains(&self, row: u32, col: u16) -> bool {
        if self.is_empty() {
            return false;
        }
        let ((sr, sc), (er, ec)) = self.normalized();
        if row < sr || row > er {
            return false;
        }
        if sr == er {
            return col >= sc && col <= ec;
        }
        if row == sr {
            return col >= sc;
        }
        if row == er {
            return col <= ec;
        }
        true
    }
}

pub fn pixel_to_cell(x: f32, y: f32, cell_w: f32, cell_h: f32) -> (u16, u16) {
    let col = (x / cell_w).floor().max(0.0) as u16;
    let row = (y / cell_h).floor().max(0.0) as u16;
    (row, col)
}

/// Convert raw mouse pixel coordinates to cell coordinates, accounting for
/// content y-offset (tab bar + title bar in transparent mode).
pub fn mouse_pixel_to_cell(
    x: f32,
    y: f32,
    cell_w: f32,
    cell_h: f32,
    content_y_offset: f32,
) -> (u16, u16) {
    pixel_to_cell(x, y - content_y_offset, cell_w, cell_h)
}

pub fn extract_text(cells: &[Vec<Cell>], selection: &Selection) -> String {
    if selection.is_empty() {
        return String::new();
    }
    let ((sr, sc), (er, ec)) = selection.normalized();
    let mut result = String::new();

    for row in sr..=er {
        let row_idx = row as usize;
        if row_idx >= cells.len() {
            break;
        }
        let line = &cells[row_idx];
        let col_start = if row == sr { sc as usize } else { 0 };
        let col_end = if row == er {
            (ec as usize + 1).min(line.len())
        } else {
            line.len()
        };

        let mut line_text = String::new();
        let mut col = col_start;
        while col < col_end {
            line_text.push(line[col].character);
            if line[col].flags.contains(CellFlags::WIDE_CHAR) {
                col += 2;
            } else {
                col += 1;
            }
        }
        let trimmed = line_text.trim_end();
        result.push_str(trimmed);

        if row < er {
            result.push('\n');
        }
    }
    result
}

/// Extract the input line text, using Ink prompt detection if available,
/// falling back to the cursor line.
/// Returns (text, prompt_row) where prompt_row is the screen row for flash.
pub fn input_line_text(grid: &growterm_grid::Grid) -> (String, u16, u16) {
    let cells = grid.cells();
    if let Some(prompt_row) = crate::ink_workaround::find_prompt_row(cells) {
        let bottom = crate::ink_workaround::find_input_bottom(cells, prompt_row);
        let mut result = String::new();
        for row_idx in prompt_row..=bottom {
            let line = &cells[row_idx];
            let mut line_text = String::new();
            let mut col = 0;
            while col < line.len() {
                let cell = &line[col];
                // Skip Ink's INVERSE cursor cell
                if cell.flags.contains(CellFlags::INVERSE) {
                    col += 1;
                    continue;
                }
                if cell.flags.contains(CellFlags::WIDE_CHAR) {
                    line_text.push(cell.character);
                    col += 2;
                } else if cell.character == '\0' {
                    line_text.push(' ');
                    col += 1;
                } else {
                    line_text.push(cell.character);
                    col += 1;
                }
            }
            let trimmed = line_text.trim_end();
            if !result.is_empty() && !trimmed.is_empty() {
                result.push('\n');
            }
            result.push_str(trimmed);
        }
        // Strip leading prompt symbol followed by space
        let result = if let Some(rest) = result.strip_prefix("❯ ")       // U+276F
            .or_else(|| result.strip_prefix("\u{276F} "))                 // U+276F explicit
            .or_else(|| result.strip_prefix("› "))                        // U+203A
            .or_else(|| result.strip_prefix("\u{203A} "))                 // U+203A explicit
            .or_else(|| result.strip_prefix("> "))                        // U+003E
            .or_else(|| result.strip_prefix("\u{276D} "))                 // U+276D ❭
            .or_else(|| result.strip_prefix("\u{BB} "))                   // U+00BB »
        {
            rest.to_string()
        } else {
            result
        };
        return (result, prompt_row as u16, bottom as u16);
    }
    let row = grid.cursor_pos().0;
    (cursor_line_text(grid), row, row)
}

/// Extract the text of the cursor line from the grid (trailing whitespace trimmed).
pub fn cursor_line_text(grid: &growterm_grid::Grid) -> String {
    let (cursor_row, _) = grid.cursor_pos();
    let cells = grid.cells();
    let row = cursor_row as usize;
    if row >= cells.len() {
        return String::new();
    }
    let line = &cells[row];
    let mut text = String::new();
    let mut col = 0;
    while col < line.len() {
        let cell = &line[col];
        if cell.flags.contains(CellFlags::WIDE_CHAR) {
            text.push(cell.character);
            col += 2;
        } else if cell.character == '\0' {
            text.push(' ');
            col += 1;
        } else {
            text.push(cell.character);
            col += 1;
        }
    }
    text.trim_end().to_string()
}

/// Extract text using absolute row coordinates from scrollback + screen cells.
pub fn extract_text_absolute(grid: &growterm_grid::Grid, selection: &Selection) -> String {
    if selection.is_empty() {
        return String::new();
    }
    let ((sr, sc), (er, ec)) = selection.normalized();
    let scrollback = grid.scrollback();
    let screen = grid.cells();
    let sb_len = scrollback.len() as u32;
    let mut result = String::new();

    for row in sr..=er {
        let line: &[Cell] = if row < sb_len {
            &scrollback[row as usize]
        } else {
            let screen_row = (row - sb_len) as usize;
            if screen_row >= screen.len() {
                break;
            }
            &screen[screen_row]
        };
        let col_start = if row == sr { sc as usize } else { 0 };
        let col_end = if row == er {
            (ec as usize + 1).min(line.len())
        } else {
            line.len()
        };

        let mut line_text = String::new();
        let mut col = col_start;
        while col < col_end {
            line_text.push(line[col].character);
            if line[col].flags.contains(CellFlags::WIDE_CHAR) {
                col += 2;
            } else {
                col += 1;
            }
        }
        let trimmed = line_text.trim_end();
        result.push_str(trimmed);

        if row < er {
            result.push('\n');
        }
    }
    result
}

/// Extract a single row's text using absolute row coordinate (scrollback + screen).
pub fn row_text_absolute(grid: &growterm_grid::Grid, abs_row: u32) -> String {
    let scrollback = grid.scrollback();
    let screen = grid.cells();
    let sb_len = scrollback.len() as u32;

    let line: &[Cell] = if abs_row < sb_len {
        &scrollback[abs_row as usize]
    } else {
        let screen_row = (abs_row - sb_len) as usize;
        if screen_row >= screen.len() {
            return String::new();
        }
        &screen[screen_row]
    };

    let mut text = String::new();
    for cell in line {
        if cell.flags.contains(CellFlags::WIDE_CHAR) {
            text.push(cell.character);
        } else if cell.character == '\0' {
            text.push(' ');
        } else {
            text.push(cell.character);
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use growterm_types::{Cell, CellFlags, Color};

    #[test]
    fn mouse_pixel_to_cell_no_offset() {
        // No title bar / tab bar offset
        assert_eq!(mouse_pixel_to_cell(15.0, 25.0, 10.0, 20.0, 0.0), (1, 1));
    }

    #[test]
    fn mouse_pixel_to_cell_with_offset() {
        // Transparent mode: title_bar(50) + tab_bar(30) = 80px offset
        // Click at y=100 → content y=20 → row 1
        assert_eq!(mouse_pixel_to_cell(15.0, 100.0, 10.0, 20.0, 80.0), (1, 1));
    }

    #[test]
    fn mouse_pixel_to_cell_click_in_header_clamps() {
        // Click at y=30, offset=80 → content y=-50 → clamped to row 0
        assert_eq!(mouse_pixel_to_cell(0.0, 30.0, 10.0, 20.0, 80.0), (0, 0));
    }

    #[test]
    fn pixel_to_cell_basic() {
        assert_eq!(pixel_to_cell(0.0, 0.0, 10.0, 20.0), (0, 0));
        assert_eq!(pixel_to_cell(15.0, 25.0, 10.0, 20.0), (1, 1));
        assert_eq!(pixel_to_cell(29.9, 59.9, 10.0, 20.0), (2, 2));
    }

    #[test]
    fn pixel_to_cell_negative_clamped() {
        assert_eq!(pixel_to_cell(-5.0, -10.0, 10.0, 20.0), (0, 0));
    }

    #[test]
    fn contains_single_row() {
        let mut sel = Selection::default();
        sel.start = (0, 2);
        sel.end = (0, 5);
        assert!(!sel.contains(0, 1));
        assert!(sel.contains(0, 2));
        assert!(sel.contains(0, 3));
        assert!(sel.contains(0, 5));
        assert!(!sel.contains(0, 6));
        assert!(!sel.contains(1, 3));
    }

    #[test]
    fn contains_multi_row() {
        let mut sel = Selection::default();
        sel.start = (1, 3);
        sel.end = (3, 2);
        assert!(!sel.contains(0, 5));
        assert!(!sel.contains(1, 2));
        assert!(sel.contains(1, 3));
        assert!(sel.contains(1, 10));
        assert!(sel.contains(2, 0));
        assert!(sel.contains(2, 50));
        assert!(sel.contains(3, 0));
        assert!(sel.contains(3, 2));
        assert!(!sel.contains(3, 3));
        assert!(!sel.contains(4, 0));
    }

    #[test]
    fn contains_reversed_selection() {
        let mut sel = Selection::default();
        sel.start = (3, 2);
        sel.end = (1, 3);
        assert!(sel.contains(1, 3));
        assert!(sel.contains(2, 0));
        assert!(sel.contains(3, 2));
        assert!(!sel.contains(3, 3));
    }

    #[test]
    fn contains_empty_selection() {
        let mut sel = Selection::default();
        sel.start = (1, 1);
        sel.end = (1, 1);
        assert!(!sel.contains(1, 1));
    }

    fn make_cells(lines: &[&str]) -> Vec<Vec<Cell>> {
        lines
            .iter()
            .map(|s| {
                s.chars()
                    .map(|c| Cell {
                        character: c,
                        fg: Color::Default,
                        bg: Color::Default,
                        flags: CellFlags::empty(),
                    })
                    .collect()
            })
            .collect()
    }

    /// Build cells like the grid does: wide chars get WIDE_CHAR flag + spacer cell
    fn make_cells_with_wide(lines: &[&str]) -> Vec<Vec<Cell>> {
        use unicode_width::UnicodeWidthChar;
        lines
            .iter()
            .map(|s| {
                let mut row = Vec::new();
                for c in s.chars() {
                    let w = UnicodeWidthChar::width(c).unwrap_or(1);
                    row.push(Cell {
                        character: c,
                        fg: Color::Default,
                        bg: Color::Default,
                        flags: if w == 2 { CellFlags::WIDE_CHAR } else { CellFlags::empty() },
                    });
                    if w == 2 {
                        row.push(Cell::default()); // spacer
                    }
                }
                row
            })
            .collect()
    }

    #[test]
    fn extract_text_single_line() {
        let cells = make_cells(&["Hello World"]);
        let mut sel = Selection::default();
        sel.start = (0, 0);
        sel.end = (0, 4);
        assert_eq!(extract_text(&cells, &sel), "Hello");
    }

    #[test]
    fn extract_text_multi_line() {
        let cells = make_cells(&["Hello  ", "World  "]);
        let mut sel = Selection::default();
        sel.start = (0, 0);
        sel.end = (1, 4);
        assert_eq!(extract_text(&cells, &sel), "Hello\nWorld");
    }

    #[test]
    fn extract_text_trims_trailing_spaces() {
        let cells = make_cells(&["Hi   "]);
        let mut sel = Selection::default();
        sel.start = (0, 0);
        sel.end = (0, 4);
        assert_eq!(extract_text(&cells, &sel), "Hi");
    }

    #[test]
    fn extract_text_empty_selection() {
        let cells = make_cells(&["Hello"]);
        let sel = Selection::default();
        assert_eq!(extract_text(&cells, &sel), "");
    }

    #[test]
    fn extract_text_wide_chars_no_spaces() {
        let cells = make_cells_with_wide(&["안녕하세요"]);
        let mut sel = Selection::default();
        sel.start = (0, 0);
        sel.end = (0, 9);
        assert_eq!(extract_text(&cells, &sel), "안녕하세요");
    }

    #[test]
    fn extract_text_mixed_ascii_and_wide() {
        let cells = make_cells_with_wide(&["Hi한글ok"]);
        let mut sel = Selection::default();
        sel.start = (0, 0);
        sel.end = (0, 7);
        assert_eq!(extract_text(&cells, &sel), "Hi한글ok");
    }

    #[test]
    fn extract_text_partial_line() {
        let cells = make_cells(&["Hello World"]);
        let mut sel = Selection::default();
        sel.start = (0, 6);
        sel.end = (0, 10);
        assert_eq!(extract_text(&cells, &sel), "World");
    }

    #[test]
    fn input_line_text_with_ink_prompt() {
        use growterm_grid::Grid;
        use growterm_types::TerminalCommand;

        let mut grid = Grid::new(80, 10);
        // Row 0: separator ─────
        for c in "─────".chars() {
            grid.apply(&TerminalCommand::Print(c));
        }
        // Move to row 1: prompt line ❯ hello
        grid.apply(&TerminalCommand::CursorPosition { row: 2, col: 1 });
        for c in "❯ hello".chars() {
            grid.apply(&TerminalCommand::Print(c));
        }
        // Row 2: separator ─────
        grid.apply(&TerminalCommand::CursorPosition { row: 3, col: 1 });
        for c in "─────".chars() {
            grid.apply(&TerminalCommand::Print(c));
        }
        // Cursor elsewhere (like Claude Code does)
        grid.apply(&TerminalCommand::CursorPosition { row: 5, col: 1 });

        let (text, flash_start, flash_end) = input_line_text(&grid);
        assert_eq!(text, "hello");
        assert_eq!(flash_start, 1); // prompt is on row 1
        assert_eq!(flash_end, 1); // single line input
    }

    #[test]
    fn input_line_text_falls_back_to_cursor_line() {
        use growterm_grid::Grid;
        use growterm_types::TerminalCommand;

        // No Ink prompt pattern, falls back to cursor_line_text
        let mut grid = Grid::new(80, 10);
        for c in "$ ls -la".chars() {
            grid.apply(&TerminalCommand::Print(c));
        }
        let (text, flash_start, flash_end) = input_line_text(&grid);
        assert_eq!(text, "$ ls -la");
        assert_eq!(flash_start, 0); // cursor is on row 0
        assert_eq!(flash_end, 0); // single line
    }

    #[test]
    fn cursor_line_text_basic() {
        use growterm_grid::Grid;
        use growterm_types::TerminalCommand;

        let mut grid = Grid::new(80, 24);
        for c in "hello world".chars() {
            grid.apply(&TerminalCommand::Print(c));
        }
        let text = cursor_line_text(&grid);
        assert_eq!(text, "hello world");
    }

    #[test]
    fn cursor_line_text_trims_trailing_spaces() {
        use growterm_grid::Grid;
        use growterm_types::TerminalCommand;

        let mut grid = Grid::new(80, 24);
        for c in "hi".chars() {
            grid.apply(&TerminalCommand::Print(c));
        }
        let text = cursor_line_text(&grid);
        assert_eq!(text, "hi");
    }

    #[test]
    fn cursor_line_text_empty_grid() {
        use growterm_grid::Grid;

        let grid = Grid::new(80, 24);
        let text = cursor_line_text(&grid);
        assert_eq!(text, "");
    }

    #[test]
    fn screen_normalized_basic() {
        let mut sel = Selection::default();
        sel.start = (10, 2);
        sel.end = (12, 5);
        // view_base=10, 24 visible rows
        let result = sel.screen_normalized(10, 24);
        assert_eq!(result, Some(((0, 2), (2, 5))));
    }

    #[test]
    fn screen_normalized_off_screen() {
        let mut sel = Selection::default();
        sel.start = (0, 0);
        sel.end = (5, 3);
        // view starts at row 10
        let result = sel.screen_normalized(10, 24);
        assert_eq!(result, None);
    }

    #[test]
    fn screen_normalized_partial_overlap() {
        let mut sel = Selection::default();
        sel.start = (8, 3);
        sel.end = (12, 5);
        // view_base=10, 24 visible rows -> selection starts before view
        let result = sel.screen_normalized(10, 24);
        assert_eq!(result, Some(((0, 0), (2, 5))));
    }
}
