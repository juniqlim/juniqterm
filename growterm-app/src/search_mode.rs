use crate::selection;

/// A single search match: absolute row, start column, end column (exclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchMatch {
    pub abs_row: u32,
    pub col_start: u16,
    pub col_end: u16,
}

pub struct SearchMode {
    pub active: bool,
    pub query: String,
    pub matches: Vec<SearchMatch>,
    /// Index into `matches` for the current highlighted match.
    pub current: usize,
}

impl SearchMode {
    pub fn new() -> Self {
        Self {
            active: false,
            query: String::new(),
            matches: Vec::new(),
            current: 0,
        }
    }

    pub fn enter(&mut self) {
        self.active = true;
        self.query.clear();
        self.matches.clear();
        self.current = 0;
    }

    pub fn exit(&mut self) {
        self.active = false;
        self.query.clear();
        self.matches.clear();
        self.current = 0;
    }

    /// Append a character to the query.
    pub fn push_char(&mut self, c: char) {
        self.query.push(c);
    }

    /// Delete the last character from the query.
    pub fn pop_char(&mut self) {
        self.query.pop();
    }

    /// Perform search across all rows (scrollback + screen) of the grid.
    /// Updates `matches` and resets `current` to the last match (closest to bottom).
    pub fn search(&mut self, grid: &growterm_grid::Grid) {
        self.matches.clear();
        self.current = 0;

        if self.query.is_empty() {
            return;
        }

        let scrollback = grid.scrollback();
        let screen = grid.cells();
        let sb_len = scrollback.len() as u32;
        let total_rows = sb_len + screen.len() as u32;

        let query_lower = self.query.to_lowercase();

        for abs_row in 0..total_rows {
            let line = if abs_row < sb_len {
                &scrollback[abs_row as usize]
            } else {
                let screen_row = (abs_row - sb_len) as usize;
                &screen[screen_row]
            };

            let text = row_text(line);
            let text_lower = text.to_lowercase();

            // Find all occurrences in this row
            let mut start = 0;
            while let Some(pos) = text_lower[start..].find(&query_lower) {
                let char_start = text[..start + pos].chars().count();
                let char_end = char_start + self.query.chars().count();
                // Convert char indices to cell columns
                let col_start = selection::char_index_to_cell_col(line, char_start) as u16;
                let col_end = selection::char_index_to_cell_col(line, char_end) as u16;
                self.matches.push(SearchMatch {
                    abs_row,
                    col_start,
                    col_end,
                });
                start += pos + query_lower.len();
            }
        }

        // Default to last match (bottom of screen)
        if !self.matches.is_empty() {
            self.current = self.matches.len() - 1;
        }
    }

    /// Move to the next match (downward). Wraps around.
    pub fn next_match(&mut self) {
        if self.matches.is_empty() {
            return;
        }
        self.current = (self.current + 1) % self.matches.len();
    }

    /// Move to the previous match (upward). Wraps around.
    pub fn prev_match(&mut self) {
        if self.matches.is_empty() {
            return;
        }
        if self.current == 0 {
            self.current = self.matches.len() - 1;
        } else {
            self.current -= 1;
        }
    }

    /// Get the current match, if any.
    pub fn current_match(&self) -> Option<&SearchMatch> {
        self.matches.get(self.current)
    }

    /// Get all match ranges as (abs_row, col_start, col_end) for rendering.
    pub fn highlight_ranges(&self) -> Vec<(u32, u16, u16)> {
        self.matches
            .iter()
            .map(|m| (m.abs_row, m.col_start, m.col_end))
            .collect()
    }
}

/// Extract text from a row of cells (for searching).
fn row_text(cells: &[growterm_types::Cell]) -> String {
    use growterm_types::CellFlags;
    let mut text = String::new();
    let mut col = 0;
    while col < cells.len() {
        let cell = &cells[col];
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

#[cfg(test)]
mod tests {
    use super::*;
    use growterm_grid::Grid;
    use growterm_types::TerminalCommand;

    fn make_grid_with_text(cols: u16, rows: u16, lines: &[&str]) -> Grid {
        let mut grid = Grid::new(cols, rows);
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                grid.apply(&TerminalCommand::Newline);
                grid.apply(&TerminalCommand::CarriageReturn);
            }
            for c in line.chars() {
                grid.apply(&TerminalCommand::Print(c));
            }
        }
        grid
    }

    #[test]
    fn enter_and_exit() {
        let mut sm = SearchMode::new();
        assert!(!sm.active);

        sm.enter();
        assert!(sm.active);
        assert!(sm.query.is_empty());

        sm.exit();
        assert!(!sm.active);
    }

    #[test]
    fn push_and_pop_char() {
        let mut sm = SearchMode::new();
        sm.enter();
        sm.push_char('h');
        sm.push_char('i');
        assert_eq!(sm.query, "hi");

        sm.pop_char();
        assert_eq!(sm.query, "h");

        sm.pop_char();
        assert_eq!(sm.query, "");

        // Pop on empty is no-op
        sm.pop_char();
        assert_eq!(sm.query, "");
    }

    #[test]
    fn search_finds_matches() {
        let grid = make_grid_with_text(80, 5, &[
            "hello world",
            "foo bar",
            "hello again",
        ]);

        let mut sm = SearchMode::new();
        sm.enter();
        sm.query = "hello".to_string();
        sm.search(&grid);

        assert_eq!(sm.matches.len(), 2);
        assert_eq!(sm.matches[0].abs_row, 0);
        assert_eq!(sm.matches[0].col_start, 0);
        assert_eq!(sm.matches[0].col_end, 5);
        assert_eq!(sm.matches[1].abs_row, 2);
        // Current defaults to last match
        assert_eq!(sm.current, 1);
    }

    #[test]
    fn search_case_insensitive() {
        let grid = make_grid_with_text(80, 5, &[
            "Hello World",
            "HELLO WORLD",
        ]);

        let mut sm = SearchMode::new();
        sm.enter();
        sm.query = "hello".to_string();
        sm.search(&grid);

        assert_eq!(sm.matches.len(), 2);
    }

    #[test]
    fn search_empty_query_no_matches() {
        let grid = make_grid_with_text(80, 5, &["hello"]);

        let mut sm = SearchMode::new();
        sm.enter();
        sm.search(&grid);

        assert!(sm.matches.is_empty());
    }

    #[test]
    fn search_no_results() {
        let grid = make_grid_with_text(80, 5, &["hello"]);

        let mut sm = SearchMode::new();
        sm.enter();
        sm.query = "xyz".to_string();
        sm.search(&grid);

        assert!(sm.matches.is_empty());
        assert!(sm.current_match().is_none());
    }

    #[test]
    fn next_and_prev_match() {
        let grid = make_grid_with_text(80, 5, &[
            "aaa",
            "aaa",
            "aaa",
        ]);

        let mut sm = SearchMode::new();
        sm.enter();
        sm.query = "aaa".to_string();
        sm.search(&grid);
        assert_eq!(sm.matches.len(), 3);
        assert_eq!(sm.current, 2); // starts at last

        sm.next_match(); // wraps to 0
        assert_eq!(sm.current, 0);

        sm.next_match();
        assert_eq!(sm.current, 1);

        sm.prev_match();
        assert_eq!(sm.current, 0);

        sm.prev_match(); // wraps to 2
        assert_eq!(sm.current, 2);
    }

    #[test]
    fn next_prev_on_empty_is_noop() {
        let mut sm = SearchMode::new();
        sm.enter();
        sm.next_match();
        sm.prev_match();
        assert_eq!(sm.current, 0);
    }

    #[test]
    fn multiple_matches_per_row() {
        let grid = make_grid_with_text(80, 5, &["abcabc"]);

        let mut sm = SearchMode::new();
        sm.enter();
        sm.query = "abc".to_string();
        sm.search(&grid);

        assert_eq!(sm.matches.len(), 2);
        assert_eq!(sm.matches[0].col_start, 0);
        assert_eq!(sm.matches[0].col_end, 3);
        assert_eq!(sm.matches[1].col_start, 3);
        assert_eq!(sm.matches[1].col_end, 6);
    }

    #[test]
    fn highlight_ranges_returns_all() {
        let grid = make_grid_with_text(80, 5, &["hello", "hello"]);

        let mut sm = SearchMode::new();
        sm.enter();
        sm.query = "hello".to_string();
        sm.search(&grid);

        let ranges = sm.highlight_ranges();
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0], (0, 0, 5));
        assert_eq!(ranges[1], (1, 0, 5));
    }

    #[test]
    fn search_in_scrollback() {
        // Create a small grid and overflow it to push lines into scrollback
        let mut grid = Grid::new(80, 3);
        for i in 0..10 {
            if i > 0 {
                grid.apply(&TerminalCommand::Newline);
                grid.apply(&TerminalCommand::CarriageReturn);
            }
            let line = format!("line{}", i);
            for c in line.chars() {
                grid.apply(&TerminalCommand::Print(c));
            }
        }

        let mut sm = SearchMode::new();
        sm.enter();
        sm.query = "line".to_string();
        sm.search(&grid);

        // Should find "line" in all rows (scrollback + screen)
        assert_eq!(sm.matches.len(), 10);
    }
}
