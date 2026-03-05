use crate::selection::Selection;

#[derive(Clone)]
pub struct CopyMode {
    pub active: bool,
    /// true = 비주얼 모드 (v로 진입, j/k로 범위 확장)
    pub visual: bool,
    /// 복사모드 커서 위치 (절대행, 열)
    pub cursor: (u32, u16),
    /// 비주얼 모드 앵커 행
    pub anchor_row: u32,
}

impl CopyMode {
    pub fn new() -> Self {
        Self {
            active: false,
            visual: false,
            cursor: (0, 0),
            anchor_row: 0,
        }
    }

    /// 복사모드 진입: 현재 커서 행 전체를 선택
    pub fn enter(&mut self, cursor_row: u32, cols: u16, sel: &mut Selection) {
        self.active = true;
        self.visual = false;
        self.cursor = (cursor_row, 0);
        self.anchor_row = cursor_row;
        self.select_line(cursor_row, cols, sel);
    }

    /// j키: 아래로 이동
    pub fn move_down(&mut self, cols: u16, max_row: u32, sel: &mut Selection) {
        if !self.active {
            return;
        }
        let new_row = (self.cursor.0 + 1).min(max_row);
        self.cursor.0 = new_row;
        if self.visual {
            self.update_visual_selection(cols, sel);
        } else {
            self.select_line(new_row, cols, sel);
        }
    }

    /// k키: 위로 이동
    pub fn move_up(&mut self, cols: u16, sel: &mut Selection) {
        if !self.active {
            return;
        }
        let new_row = self.cursor.0.saturating_sub(1);
        self.cursor.0 = new_row;
        if self.visual {
            self.update_visual_selection(cols, sel);
        } else {
            self.select_line(new_row, cols, sel);
        }
    }

    /// v키: 비주얼 모드 토글 (현재 위치부터 범위 확장)
    pub fn toggle_visual(&mut self, cols: u16, sel: &mut Selection) {
        if !self.active {
            return;
        }
        self.visual = !self.visual;
        if self.visual {
            self.anchor_row = self.cursor.0;
            self.select_line(self.cursor.0, cols, sel);
        } else {
            // 비주얼 해제 → 현재 커서 행만 선택
            self.select_line(self.cursor.0, cols, sel);
        }
    }

    /// h키: 10줄 위로 이동
    pub fn move_left(&mut self, cols: u16, sel: &mut Selection) {
        if !self.active {
            return;
        }
        let new_row = self.cursor.0.saturating_sub(10);
        self.cursor.0 = new_row;
        if self.visual {
            self.update_visual_selection(cols, sel);
        } else {
            self.select_line(new_row, cols, sel);
        }
    }

    /// l키: 10줄 아래로 이동
    pub fn move_right(&mut self, cols: u16, max_row: u32, sel: &mut Selection) {
        if !self.active {
            return;
        }
        let new_row = (self.cursor.0 + 10).min(max_row);
        self.cursor.0 = new_row;
        if self.visual {
            self.update_visual_selection(cols, sel);
        } else {
            self.select_line(new_row, cols, sel);
        }
    }

    /// 복사모드 종료
    pub fn exit(&mut self, sel: &mut Selection) {
        self.active = false;
        self.visual = false;
        sel.clear();
    }

    fn select_line(&self, row: u32, cols: u16, sel: &mut Selection) {
        sel.start = (row, 0);
        sel.end = (row, cols - 1);
    }

    fn update_visual_selection(&self, cols: u16, sel: &mut Selection) {
        let (start_row, end_row) = if self.cursor.0 >= self.anchor_row {
            (self.anchor_row, self.cursor.0)
        } else {
            (self.cursor.0, self.anchor_row)
        };
        sel.start = (start_row, 0);
        sel.end = (end_row, cols - 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLS: u16 = 80;

    #[test]
    fn enter_selects_current_line() {
        let mut cm = CopyMode::new();
        let mut sel = Selection::default();
        cm.enter(5, COLS, &mut sel);

        assert!(cm.active);
        assert!(!cm.visual);
        assert_eq!(sel.start, (5, 0));
        assert_eq!(sel.end, (5, COLS - 1));
    }

    #[test]
    fn j_moves_highlight_down() {
        let mut cm = CopyMode::new();
        let mut sel = Selection::default();
        cm.enter(5, COLS, &mut sel);

        cm.move_down(COLS, 100, &mut sel);
        // 단일 행 이동: 6번 행만 선택
        assert_eq!(sel.start, (6, 0));
        assert_eq!(sel.end, (6, COLS - 1));
    }

    #[test]
    fn k_moves_highlight_up() {
        let mut cm = CopyMode::new();
        let mut sel = Selection::default();
        cm.enter(5, COLS, &mut sel);

        cm.move_up(COLS, &mut sel);
        assert_eq!(sel.start, (4, 0));
        assert_eq!(sel.end, (4, COLS - 1));
    }

    #[test]
    fn j_then_k_returns_to_original() {
        let mut cm = CopyMode::new();
        let mut sel = Selection::default();
        cm.enter(5, COLS, &mut sel);

        cm.move_down(COLS, 100, &mut sel);
        cm.move_up(COLS, &mut sel);
        assert_eq!(sel.start, (5, 0));
        assert_eq!(sel.end, (5, COLS - 1));
    }

    #[test]
    fn j_clamps_at_max_row() {
        let mut cm = CopyMode::new();
        let mut sel = Selection::default();
        cm.enter(10, COLS, &mut sel);

        cm.move_down(COLS, 10, &mut sel);
        assert_eq!(cm.cursor.0, 10);
    }

    #[test]
    fn k_clamps_at_zero() {
        let mut cm = CopyMode::new();
        let mut sel = Selection::default();
        cm.enter(0, COLS, &mut sel);

        cm.move_up(COLS, &mut sel);
        assert_eq!(cm.cursor.0, 0);
    }

    #[test]
    fn v_enters_visual_mode() {
        let mut cm = CopyMode::new();
        let mut sel = Selection::default();
        cm.enter(5, COLS, &mut sel);

        cm.toggle_visual(COLS, &mut sel);
        assert!(cm.visual);
    }

    #[test]
    fn visual_j_extends_selection() {
        let mut cm = CopyMode::new();
        let mut sel = Selection::default();
        cm.enter(5, COLS, &mut sel);

        cm.toggle_visual(COLS, &mut sel);
        cm.move_down(COLS, 100, &mut sel);
        // 비주얼: 5~6행 선택
        assert_eq!(sel.start, (5, 0));
        assert_eq!(sel.end, (6, COLS - 1));

        cm.move_down(COLS, 100, &mut sel);
        assert_eq!(sel.end, (7, COLS - 1));
    }

    #[test]
    fn visual_k_extends_selection_up() {
        let mut cm = CopyMode::new();
        let mut sel = Selection::default();
        cm.enter(5, COLS, &mut sel);

        cm.toggle_visual(COLS, &mut sel);
        cm.move_up(COLS, &mut sel);
        assert_eq!(sel.start, (4, 0));
        assert_eq!(sel.end, (5, COLS - 1));
    }

    #[test]
    fn v_toggle_off_returns_to_single_line() {
        let mut cm = CopyMode::new();
        let mut sel = Selection::default();
        cm.enter(5, COLS, &mut sel);

        cm.toggle_visual(COLS, &mut sel);
        cm.move_down(COLS, 100, &mut sel);
        cm.move_down(COLS, 100, &mut sel);
        // 비주얼 해제 → 현재 커서 행(7)만 선택
        cm.toggle_visual(COLS, &mut sel);
        assert!(!cm.visual);
        assert_eq!(sel.start, (7, 0));
        assert_eq!(sel.end, (7, COLS - 1));
    }

    #[test]
    fn h_moves_10_lines_up() {
        let mut cm = CopyMode::new();
        let mut sel = Selection::default();
        cm.enter(20, COLS, &mut sel);

        cm.move_left(COLS, &mut sel);
        assert_eq!(cm.cursor.0, 10);
        assert_eq!(sel.start, (10, 0));
        assert_eq!(sel.end, (10, COLS - 1));
    }

    #[test]
    fn l_moves_10_lines_down() {
        let mut cm = CopyMode::new();
        let mut sel = Selection::default();
        cm.enter(10, COLS, &mut sel);

        cm.move_right(COLS, 100, &mut sel);
        assert_eq!(cm.cursor.0, 20);
        assert_eq!(sel.start, (20, 0));
        assert_eq!(sel.end, (20, COLS - 1));
    }

    #[test]
    fn h_clamps_at_zero() {
        let mut cm = CopyMode::new();
        let mut sel = Selection::default();
        cm.enter(3, COLS, &mut sel);

        cm.move_left(COLS, &mut sel);
        assert_eq!(cm.cursor.0, 0);
    }

    #[test]
    fn l_clamps_at_max_row() {
        let mut cm = CopyMode::new();
        let mut sel = Selection::default();
        cm.enter(98, COLS, &mut sel);

        cm.move_right(COLS, 100, &mut sel);
        assert_eq!(cm.cursor.0, 100);
    }

    #[test]
    fn visual_h_l_extend_selection() {
        let mut cm = CopyMode::new();
        let mut sel = Selection::default();
        cm.enter(10, COLS, &mut sel);
        cm.toggle_visual(COLS, &mut sel);

        cm.move_right(COLS, 100, &mut sel);
        assert_eq!(sel.start, (10, 0));
        assert_eq!(sel.end, (20, COLS - 1));

        cm.move_left(COLS, &mut sel);
        assert_eq!(sel.start, (10, 0));
        assert_eq!(sel.end, (10, COLS - 1));
    }

    #[test]
    fn exit_clears_selection() {
        let mut cm = CopyMode::new();
        let mut sel = Selection::default();
        cm.enter(5, COLS, &mut sel);

        cm.exit(&mut sel);
        assert!(!cm.active);
        assert!(sel.is_empty());
    }

    #[test]
    fn inactive_operations_are_no_op() {
        let mut cm = CopyMode::new();
        let mut sel = Selection::default();

        cm.move_down(COLS, 100, &mut sel);
        cm.move_up(COLS, &mut sel);
        cm.move_left(COLS, &mut sel);
        cm.move_right(COLS, 100, &mut sel);
        cm.toggle_visual(COLS, &mut sel);

        assert!(!cm.active);
        assert!(sel.is_empty());
    }
}
