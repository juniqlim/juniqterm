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

/// Extract text from a slice of cells, skipping wide-char spacer cells.
fn collect_cells_text(line: &[Cell], col_start: usize, col_end: usize) -> String {
    let mut text = String::new();
    let mut col = col_start;
    while col < col_end {
        text.push(line[col].character);
        if line[col].flags.contains(CellFlags::WIDE_CHAR) {
            col += 2;
        } else {
            col += 1;
        }
    }
    text
}

/// Extract full line text, replacing null chars with spaces and skipping wide-char spacers.
fn collect_line_text(line: &[Cell]) -> String {
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
    text
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

        let line_text = collect_cells_text(line, col_start, col_end);
        result.push_str(line_text.trim_end());

        if row < er {
            // Skip newline for soft-wrapped rows (last cell is non-space/non-null)
            if !is_row_wrapped(line) {
                result.push('\n');
            }
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
    collect_line_text(&cells[row]).trim_end().to_string()
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

        let line_text = collect_cells_text(line, col_start, col_end);
        result.push_str(line_text.trim_end());

        if row < er {
            if !is_row_wrapped(line) {
                result.push('\n');
            }
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

    collect_line_text(line)
}

/// Convert cell column (wide char = 2 cols) to char index (wide char = 1).
pub fn cell_col_to_char_index(line: &[Cell], cell_col: usize) -> usize {
    let mut char_idx = 0;
    let mut col = 0;
    while col < line.len() && col < cell_col {
        if line[col].flags.contains(CellFlags::WIDE_CHAR) {
            col += 2;
        } else {
            col += 1;
        }
        char_idx += 1;
    }
    char_idx
}

/// Convert char index (wide char = 1) to cell column (wide char = 2 cols).
pub fn char_index_to_cell_col(line: &[Cell], char_idx: usize) -> usize {
    let mut col = 0;
    let mut idx = 0;
    while col < line.len() && idx < char_idx {
        if line[col].flags.contains(CellFlags::WIDE_CHAR) {
            col += 2;
        } else {
            col += 1;
        }
        idx += 1;
    }
    col
}

/// Check if a row looks like it was soft-wrapped (last cell is non-space/non-null).
fn is_row_wrapped(cells: &[Cell]) -> bool {
    if cells.is_empty() {
        return false;
    }
    let last = &cells[cells.len() - 1];
    last.character != ' ' && last.character != '\0'
}

/// Find the range of rows forming a logical line around `abs_row`.
fn logical_line_rows(grid: &growterm_grid::Grid, abs_row: u32) -> (u32, u32) {
    let sb_len = grid.scrollback().len() as u32;
    let total_rows = sb_len + grid.cells().len() as u32;

    let mut first = abs_row;
    while first > 0 {
        let prev_cells = row_cells_absolute(grid, first - 1);
        if is_row_wrapped(&prev_cells) {
            first -= 1;
        } else {
            break;
        }
    }

    let mut last = abs_row;
    loop {
        let cells = row_cells_absolute(grid, last);
        if last >= total_rows.saturating_sub(1) || !is_row_wrapped(&cells) {
            break;
        }
        last += 1;
    }

    (first, last)
}

/// Build a URL-friendly logical line by stripping leading whitespace from continuation rows.
/// Returns (stripped_text, logical_col) where logical_col is the adjusted column in stripped_text.
fn build_url_logical_line(
    grid: &growterm_grid::Grid,
    abs_row: u32,
    cell_col: usize,
) -> (String, usize) {
    let (first, last) = logical_line_rows(grid, abs_row);

    let mut stripped_text = String::new();
    let mut logical_col = 0;

    for row in first..=last {
        let cells = row_cells_absolute(grid, row);
        let text = collect_line_text(&cells);

        let (effective, leading_chars) = if row == first {
            (text.as_str(), 0usize)
        } else {
            let trimmed = text.trim_start();
            let trim_bytes = text.len() - trimmed.len();
            let trim_chars = text[..trim_bytes].chars().count();
            (trimmed, trim_chars)
        };

        if row == abs_row {
            let char_col_in_row = cell_col_to_char_index(&cells, cell_col);
            logical_col = stripped_text.chars().count()
                + char_col_in_row.saturating_sub(leading_chars);
        }

        stripped_text.push_str(effective);

        if row == last {
            break;
        }
    }

    (stripped_text, logical_col)
}

/// Find a URL at the given grid position, joining wrapped/broken lines.
pub fn find_url_at_logical(
    grid: &growterm_grid::Grid,
    abs_row: u32,
    cell_col: usize,
) -> Option<String> {
    let (text, col) = build_url_logical_line(grid, abs_row, cell_col);
    crate::url::find_url_at(&text, col).map(|s| s.to_string())
}

/// Compute per-row hover underline ranges for a URL at the given position.
/// Walks through URL chars and row chars in parallel, skipping non-matching whitespace.
pub fn find_url_hover_ranges(
    grid: &growterm_grid::Grid,
    abs_row: u32,
    cell_col: usize,
) -> Vec<(u32, u16, u16)> {
    let url = match find_url_at_logical(grid, abs_row, cell_col) {
        Some(u) => u,
        None => return Vec::new(),
    };

    let (first, last) = logical_line_rows(grid, abs_row);
    let mut ranges = Vec::new();
    let mut url_chars = url.chars().peekable();

    for row in first..=last {
        if url_chars.peek().is_none() {
            break;
        }

        let cells = row_cells_absolute(grid, row);
        let text = collect_line_text(&cells);
        let text_chars: Vec<char> = text.chars().collect();

        let mut start_char_idx: Option<usize> = None;
        let mut end_char_idx: usize = 0;

        for (i, &tc) in text_chars.iter().enumerate() {
            if let Some(&uc) = url_chars.peek() {
                if tc == uc {
                    if start_char_idx.is_none() {
                        start_char_idx = Some(i);
                    }
                    end_char_idx = i + 1;
                    url_chars.next();
                }
                // skip whitespace/non-matching chars (indentation)
            } else {
                break;
            }
        }

        if let Some(sci) = start_char_idx {
            let start_cell = char_index_to_cell_col(&cells, sci) as u16;
            let end_cell = char_index_to_cell_col(&cells, end_char_idx) as u16;
            ranges.push((row, start_cell, end_cell));
        }
    }

    ranges
}

/// Build a logical line by concatenating soft-wrapped rows around `abs_row`.
/// Returns (combined_text, combined_cells, first_abs_row, char_offset_of_target_row).
pub fn build_logical_line(
    grid: &growterm_grid::Grid,
    abs_row: u32,
) -> (String, Vec<Cell>, u32, usize) {
    let (first, last) = logical_line_rows(grid, abs_row);

    let mut combined_text = String::new();
    let mut combined_cells: Vec<Cell> = Vec::new();
    let mut char_offset = 0;

    for row in first..=last {
        let cells = row_cells_absolute(grid, row);
        let text = collect_line_text(&cells);
        if row == abs_row {
            char_offset = combined_text.chars().count();
        }
        combined_text.push_str(&text);
        combined_cells.extend(cells.iter().cloned());
    }

    (combined_text, combined_cells, first, char_offset)
}

/// Convert a char range in a logical (multi-row) line back to per-row (abs_row, start_cell_col, end_cell_col).
pub fn logical_range_to_row_ranges(
    logical_cells: &[Cell],
    first_row: u32,
    cols: usize,
    char_start: usize,
    char_end: usize,
) -> Vec<(u32, u16, u16)> {
    // Walk logical_cells to map char indices to (row, cell_col)
    let mut result = Vec::new();
    let mut char_idx = 0;
    let mut cell_idx = 0;

    // Find the cell index for char_start and char_end
    let mut start_cell = 0;
    let mut end_cell = 0;
    while cell_idx < logical_cells.len() && char_idx < char_end {
        if char_idx == char_start {
            start_cell = cell_idx;
        }
        if logical_cells[cell_idx].flags.contains(CellFlags::WIDE_CHAR) {
            cell_idx += 2;
        } else {
            cell_idx += 1;
        }
        char_idx += 1;
        if char_idx == char_end {
            end_cell = cell_idx;
        }
    }
    if char_idx == char_start {
        start_cell = cell_idx;
    }
    if char_idx >= char_end && end_cell == 0 {
        end_cell = cell_idx;
    }

    // Now split the cell range [start_cell..end_cell] into per-row segments
    let first_row_of_range = start_cell / cols;
    let last_row_of_range = if end_cell == 0 { 0 } else { (end_cell - 1) / cols };

    for row_offset in first_row_of_range..=last_row_of_range {
        let row_start_cell = row_offset * cols;
        let row_end_cell = (row_offset + 1) * cols;
        let seg_start = start_cell.max(row_start_cell) - row_start_cell;
        let seg_end = end_cell.min(row_end_cell) - row_start_cell;
        if seg_start < seg_end {
            result.push((first_row + row_offset as u32, seg_start as u16, seg_end as u16));
        }
    }

    result
}

/// Get cell slice for an absolute row (scrollback + screen).
pub fn row_cells_absolute(grid: &growterm_grid::Grid, abs_row: u32) -> Vec<Cell> {
    let scrollback = grid.scrollback();
    let screen = grid.cells();
    let sb_len = scrollback.len() as u32;
    if abs_row < sb_len {
        scrollback[abs_row as usize].clone()
    } else {
        let screen_row = (abs_row - sb_len) as usize;
        if screen_row >= screen.len() {
            Vec::new()
        } else {
            screen[screen_row].clone()
        }
    }
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
    fn extract_text_soft_wrapped_lines_no_newline() {
        // Simulate a 10-col terminal with "hello world!" wrapped across 2 rows
        // Row 0: "helloworld" (last char non-space → wrapped)
        // Row 1: "!         "
        let cells = make_cells(&["helloworld", "!         "]);
        let mut sel = Selection::default();
        sel.start = (0, 0);
        sel.end = (1, 0);
        sel.active = true;
        // Should NOT have a newline between wrapped rows
        assert_eq!(extract_text(&cells, &sel), "helloworld!");
    }

    #[test]
    fn extract_text_hard_newline_keeps_newline() {
        // Row 0: "hello     " (last char is space → not wrapped, real newline)
        // Row 1: "world     "
        let cells = make_cells(&["hello     ", "world     "]);
        let mut sel = Selection::default();
        sel.start = (0, 0);
        sel.end = (1, 4);
        sel.active = true;
        assert_eq!(extract_text(&cells, &sel), "hello\nworld");
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

    #[test]
    fn cell_col_to_char_index_ascii_only() {
        // "hello" → all width-1, cell col == char index
        let cells = make_cells_with_wide(&["hello"]);
        assert_eq!(cell_col_to_char_index(&cells[0], 0), 0);
        assert_eq!(cell_col_to_char_index(&cells[0], 3), 3);
    }

    #[test]
    fn cell_col_to_char_index_with_wide() {
        // "한글 hi" → cells: [한][spacer][글][spacer][ ][h][i]
        // cell cols:          0    1       2    3     4   5  6
        // char idx:           0            1         2   3  4
        let cells = make_cells_with_wide(&["한글 hi"]);
        assert_eq!(cell_col_to_char_index(&cells[0], 0), 0); // '한'
        assert_eq!(cell_col_to_char_index(&cells[0], 2), 1); // '글'
        assert_eq!(cell_col_to_char_index(&cells[0], 4), 2); // ' '
        assert_eq!(cell_col_to_char_index(&cells[0], 5), 3); // 'h'
        assert_eq!(cell_col_to_char_index(&cells[0], 6), 4); // 'i'
    }

    #[test]
    fn char_index_to_cell_col_ascii_only() {
        let cells = make_cells_with_wide(&["hello"]);
        assert_eq!(char_index_to_cell_col(&cells[0], 0), 0);
        assert_eq!(char_index_to_cell_col(&cells[0], 3), 3);
    }

    #[test]
    fn char_index_to_cell_col_with_wide() {
        // "한글 hi" → char 0='한' at col 0, char 1='글' at col 2, char 2=' ' at col 4
        let cells = make_cells_with_wide(&["한글 hi"]);
        assert_eq!(char_index_to_cell_col(&cells[0], 0), 0);
        assert_eq!(char_index_to_cell_col(&cells[0], 1), 2);
        assert_eq!(char_index_to_cell_col(&cells[0], 2), 4);
        assert_eq!(char_index_to_cell_col(&cells[0], 3), 5);
        assert_eq!(char_index_to_cell_col(&cells[0], 4), 6);
    }

    #[test]
    fn is_row_wrapped_non_space_end() {
        let cells = make_cells(&["abcde"]);
        assert!(is_row_wrapped(&cells[0]));
    }

    #[test]
    fn is_row_wrapped_space_end() {
        let cells = make_cells(&["abc  "]);
        assert!(!is_row_wrapped(&cells[0]));
    }

    #[test]
    fn is_row_wrapped_empty() {
        assert!(!is_row_wrapped(&[]));
    }

    #[test]
    fn logical_range_to_row_ranges_single_row() {
        // 10 cols per row, URL "https://x.c" at chars 0..11 on single row
        let cells = make_cells(&["https://x.c"]);
        let result = logical_range_to_row_ranges(&cells[0], 5, 11, 0, 11);
        assert_eq!(result, vec![(5, 0, 11)]);
    }

    #[test]
    fn logical_range_to_row_ranges_two_rows() {
        // 5 cols per row, logical line = "abchttps://x" (12 chars, 12 cells)
        // Row 0 (first_row=10): cells 0..5 = "abcht"
        // Row 1 (first_row=11): cells 5..10 = "tps:/"
        // Row 2 (first_row=12): cells 10..12 = "/x"
        // URL "https://x" starts at char 3, ends at char 12 → cells 3..12
        let mut all_cells: Vec<Cell> = Vec::new();
        for c in "abchttps://x".chars() {
            all_cells.push(Cell {
                character: c,
                fg: Color::Default,
                bg: Color::Default,
                flags: CellFlags::empty(),
            });
        }
        let result = logical_range_to_row_ranges(&all_cells, 10, 5, 3, 12);
        // Row 10: cells 3..5
        // Row 11: cells 0..5
        // Row 12: cells 0..2
        assert_eq!(result, vec![(10, 3, 5), (11, 0, 5), (12, 0, 2)]);
    }

    #[test]
    fn build_logical_line_wrapped_url() {
        use growterm_grid::Grid;
        use growterm_types::TerminalCommand;

        // 10-col terminal, print "https://example.com/path" (24 chars, wraps across 3 rows)
        let mut grid = Grid::new(10, 5);
        for c in "https://example.com/path".chars() {
            grid.apply(&TerminalCommand::Print(c));
        }
        // Row 0: "https://ex" (wrapped)
        // Row 1: "ample.com/" (wrapped)
        // Row 2: "path      "

        let (text, _cells, first_row, offset) = build_logical_line(&grid, 0);
        assert!(text.starts_with("https://ex"));
        assert_eq!(first_row, 0);
        assert_eq!(offset, 0);

        // Clicking on row 1 should also find the full URL
        let (text1, _cells1, first_row1, offset1) = build_logical_line(&grid, 1);
        assert_eq!(first_row1, 0);
        assert_eq!(offset1, 10); // 10 chars in row 0
        assert!(text1.contains("https://"));
    }

    #[test]
    fn wrapped_url_find_url_at_from_any_row() {
        use growterm_grid::Grid;
        use growterm_types::TerminalCommand;
        use crate::url;

        // 20-col terminal, URL = "https://example.com/very/long/path/here" (39 chars)
        // Row 0: "https://example.com/" (20 chars, wrapped)
        // Row 1: "very/long/path/here" + null cell (19 chars + 1 null → ' ')
        let url_str = "https://example.com/very/long/path/here";
        let mut grid = Grid::new(20, 5);
        for c in url_str.chars() {
            grid.apply(&TerminalCommand::Print(c));
        }

        let (text, _cells, _first, offset) = build_logical_line(&grid, 0);

        // From row 0, col 5 (inside URL)
        let logical_col = offset + 5;
        let found = url::find_url_at(&text, logical_col);
        assert_eq!(found, Some(url_str));

        // From row 1, col 3 (inside "very/long/..." part)
        let (text1, _cells1, _first1, offset1) = build_logical_line(&grid, 1);
        let logical_col1 = offset1 + 3;
        let found1 = url::find_url_at(&text1, logical_col1);
        assert_eq!(found1, Some(url_str));
    }

    #[test]
    fn hard_break_yaml_url_click() {
        use growterm_grid::Grid;
        use growterm_types::TerminalCommand;

        // Simulate 80-col terminal displaying YAML with URLs broken by newline+indent.
        // The YAML serializer inserts \n followed by 2-space indent in long URLs.
        let mut grid = Grid::new(80, 30);

        // This simulates `cat file.yaml` output where URLs are split across lines.
        // Line 1: "  - https://namu.wiki/w/%EB%A5%B4...%20%E" (exactly 80 chars, fills row)
        // Line 2: "  C%82%AC%EB%A7%9D%20%EC%82%AC%EA%B1%B4" (starts with 2-space indent)
        let line1 = "  - https://namu.wiki/w/%EB%A5%B4%EB%84%A4%20%EB%8B%88%EC%BD%9C%20%EA%B5%BF%20%E";
        let line2 = "  C%82%AC%EB%A7%9D%20%EC%82%AC%EA%B1%B4";

        assert_eq!(line1.len(), 80); // verify it fills the terminal width

        for c in line1.chars() {
            grid.apply(&TerminalCommand::Print(c));
        }
        // Hard line break (actual \n in content)
        grid.apply(&TerminalCommand::Newline);
        grid.apply(&TerminalCommand::CarriageReturn);
        for c in line2.chars() {
            grid.apply(&TerminalCommand::Print(c));
        }

        let expected_url = "https://namu.wiki/w/%EB%A5%B4%EB%84%A4%20%EB%8B%88%EC%BD%9C%20%EA%B5%BF%20%EC%82%AC%EB%A7%9D%20%EC%82%AC%EA%B1%B4";

        // Click on row 0, col 10 (inside URL first part)
        let found0 = find_url_at_logical(&grid, 0, 10);
        assert_eq!(found0.as_deref(), Some(expected_url));

        // Click on row 1, col 5 (inside URL continuation, after 2-space indent)
        let found1 = find_url_at_logical(&grid, 1, 5);
        assert_eq!(found1.as_deref(), Some(expected_url));

        // Hover: should produce ranges on both rows
        let ranges = find_url_hover_ranges(&grid, 1, 5);
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].0, 0); // row 0
        assert_eq!(ranges[0].1, 4); // starts after "  - "
        assert_eq!(ranges[1].0, 1); // row 1
        assert_eq!(ranges[1].1, 2); // starts after "  " indent
    }

    #[test]
    fn hard_break_yaml_all_urls() {
        use growterm_grid::Grid;
        use growterm_types::TerminalCommand;

        let mut grid = Grid::new(80, 30);

        // Print content line by line with actual newlines (simulates `cat` output)
        let content_lines = [
            "  Sources:",
            "  - https://en.wikipedia.org/wiki/Operation_Metro_Surge",
            "  - https://namu.wiki/w/%EB%A5%B4%EB%84%A4%20%EB%8B%88%EC%BD%9C%20%EA%B5%BF%20%E",
            "  C%82%AC%EB%A7%9D%20%EC%82%AC%EA%B1%B4",
            "  - https://www.minneapolismn.gov/news/2026/january/ag-lawsuit/",
            "  - https://www.shadedcommunity.com/2026/02/26/%EB%AF%B8%EB%84%A4%EC%86%8C%ED%83",
            "  %80-ice-%EC%84%9C%EB%A5%98-%EC%95%95%EC%88%98-%EB%85%BC%EB%9E%80/",
            "  - https://imnews.imbc.com/news/2026/world/article/6794374_36925.html",
            "  - https://namu.wiki/w/2026%EB%85%84%20%EC%9D%B4%EB%AF%BC%EC%84%B8%EA%B4%80%EB%",
            "  8B%A8%EC%86%8D%EA%B5%AD%20%EB%B0%98%EB%8C%80%20%EC%8B%9C%EC%9C%84",
        ];

        for (i, line) in content_lines.iter().enumerate() {
            if i > 0 {
                grid.apply(&TerminalCommand::Newline);
                grid.apply(&TerminalCommand::CarriageReturn);
            }
            for c in line.chars() {
                grid.apply(&TerminalCommand::Print(c));
            }
        }

        let namu1_url = "https://namu.wiki/w/%EB%A5%B4%EB%84%A4%20%EB%8B%88%EC%BD%9C%20%EA%B5%BF%20%EC%82%AC%EB%A7%9D%20%EC%82%AC%EA%B1%B4";
        let shaded_url = "https://www.shadedcommunity.com/2026/02/26/%EB%AF%B8%EB%84%A4%EC%86%8C%ED%83%80-ice-%EC%84%9C%EB%A5%98-%EC%95%95%EC%88%98-%EB%85%BC%EB%9E%80/";
        let namu2_url = "https://namu.wiki/w/2026%EB%85%84%20%EC%9D%B4%EB%AF%BC%EC%84%B8%EA%B4%80%EB%8B%A8%EC%86%8D%EA%B5%AD%20%EB%B0%98%EB%8C%80%20%EC%8B%9C%EC%9C%84";

        // Row 3 = "  C%82%AC..." (continuation of namu1)
        assert_eq!(find_url_at_logical(&grid, 3, 5).as_deref(), Some(namu1_url));
        // Row 2 = "  - https://namu..." (first part of namu1)
        assert_eq!(find_url_at_logical(&grid, 2, 10).as_deref(), Some(namu1_url));

        // Row 6 = "  %80-ice-..." (continuation of shaded)
        assert_eq!(find_url_at_logical(&grid, 6, 5).as_deref(), Some(shaded_url));
        // Row 5 = "  - https://www.shadedcommunity..." (first part of shaded)
        assert_eq!(find_url_at_logical(&grid, 5, 10).as_deref(), Some(shaded_url));

        // Row 9 = "  8B%A8..." (continuation of namu2)
        assert_eq!(find_url_at_logical(&grid, 9, 5).as_deref(), Some(namu2_url));
        // Row 8 = "  - https://namu.wiki/w/2026..." (first part of namu2)
        assert_eq!(find_url_at_logical(&grid, 8, 10).as_deref(), Some(namu2_url));
    }

    #[test]
    fn soft_wrap_url_hover_ranges() {
        use growterm_grid::Grid;
        use growterm_types::TerminalCommand;

        // Pure soft-wrap (no newline in content)
        let url_str = "https://example.com/very/long/path/here";
        let mut grid = Grid::new(20, 5);
        for c in url_str.chars() {
            grid.apply(&TerminalCommand::Print(c));
        }

        let ranges = find_url_hover_ranges(&grid, 1, 3);
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].0, 0);
        assert_eq!(ranges[1].0, 1);
    }
}
