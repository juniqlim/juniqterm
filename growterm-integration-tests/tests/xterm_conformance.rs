//! xterm conformance tests
//!
//! Each test sends VT sequences and verifies Grid state matches xterm behavior.
//! Reference: xterm control sequences documentation, vttest expected outputs.

use growterm_grid::Grid;
use growterm_types::{CellFlags, Color, Rgb};
use growterm_vt_parser::VtParser;

fn vt(input: &[u8]) -> Grid {
    vt_sized(input, 80, 24)
}

fn vt_sized(input: &[u8], cols: u16, rows: u16) -> Grid {
    let mut parser = VtParser::new();
    let cmds = parser.parse(input);
    let mut grid = Grid::new(cols, rows);
    for cmd in &cmds {
        grid.apply(cmd);
    }
    grid
}

fn row_text(grid: &Grid, row: usize) -> String {
    grid.cells()[row]
        .iter()
        .map(|c| c.character)
        .collect::<String>()
        .trim_end()
        .to_string()
}

// ============================================================
// 1. Cursor Movement (CUU, CUD, CUF, CUB, CUP, HPA, VPA)
// ============================================================

#[test]
fn cup_absolute_positioning() {
    // CSI row;col H — 1-based. xterm moves cursor to (row-1, col-1) in 0-based.
    let grid = vt(b"\x1b[5;10HX");
    assert_eq!(grid.cells()[4][9].character, 'X');
    assert_eq!(grid.cursor_pos(), (4, 10));
}

#[test]
fn cup_default_params_go_to_home() {
    // CSI H with no params = CSI 1;1 H = home position
    let grid = vt(b"\x1b[5;5Hx\x1b[HY");
    assert_eq!(grid.cells()[0][0].character, 'Y');
    assert_eq!(grid.cursor_pos(), (0, 1));
}

#[test]
fn cuu_stops_at_top() {
    // CSI 999 A — cursor up 999 from row 2 should stop at row 0
    let grid = vt(b"\x1b[3;1H\x1b[999AX");
    assert_eq!(grid.cells()[0][0].character, 'X');
}

#[test]
fn cud_stops_at_bottom() {
    // CSI 999 B — cursor down 999 should stop at last row
    let grid = vt(b"\x1b[999BX");
    assert_eq!(grid.cells()[23][0].character, 'X');
}

#[test]
fn cuf_stops_at_right_edge() {
    // CSI 999 C — cursor forward 999 should stop at last column
    let grid = vt(b"\x1b[999CX");
    assert_eq!(grid.cells()[0][79].character, 'X');
}

#[test]
fn cub_stops_at_left_edge() {
    // CSI 999 D — cursor back 999 from col 5 should stop at col 0
    let grid = vt(b"\x1b[1;6H\x1b[999DX");
    assert_eq!(grid.cells()[0][0].character, 'X');
}

#[test]
fn hpa_cursor_column() {
    // CSI n G — cursor to column n (1-based)
    let grid = vt(b"\x1b[20GX");
    assert_eq!(grid.cells()[0][19].character, 'X');
}

#[test]
fn vpa_cursor_row() {
    // CSI n d — cursor to row n (1-based), column unchanged
    let grid = vt(b"\x1b[5G\x1b[10dX");
    assert_eq!(grid.cells()[9][4].character, 'X');
}

// ============================================================
// 2. Erase operations (ED, EL, ECH)
// ============================================================

#[test]
fn el0_erase_from_cursor_to_end_of_line() {
    // CSI 0 K — erase from cursor to end of line
    let grid = vt(b"ABCDEF\x1b[1;4H\x1b[0K");
    assert_eq!(row_text(&grid, 0), "ABC");
}

#[test]
fn el1_erase_from_start_to_cursor() {
    // CSI 1 K — erase from start of line to cursor (inclusive)
    let grid = vt(b"ABCDEF\x1b[1;4H\x1b[1K");
    assert_eq!(grid.cells()[0][0].character, ' ');
    assert_eq!(grid.cells()[0][1].character, ' ');
    assert_eq!(grid.cells()[0][2].character, ' ');
    assert_eq!(grid.cells()[0][3].character, ' ');
    assert_eq!(grid.cells()[0][4].character, 'E');
}

#[test]
fn el2_erase_entire_line() {
    // CSI 2 K — erase entire line
    let grid = vt(b"ABCDEF\x1b[1;4H\x1b[2K");
    assert_eq!(row_text(&grid, 0), "");
}

#[test]
fn ed0_erase_below() {
    // CSI 0 J — erase from cursor to end of screen
    let grid = vt(b"\x1b[1;1HROW1\x1b[2;1HROW2\x1b[3;1HROW3\x1b[2;1H\x1b[0J");
    assert_eq!(row_text(&grid, 0), "ROW1");
    assert_eq!(row_text(&grid, 1), "");
    assert_eq!(row_text(&grid, 2), "");
}

#[test]
fn ed1_erase_above() {
    // CSI 1 J — erase from start of screen to cursor (inclusive)
    let grid = vt(b"\x1b[1;1HROW1\x1b[2;1HROW2\x1b[3;1HROW3\x1b[2;3H\x1b[1J");
    assert_eq!(row_text(&grid, 0), "");          // row above cursor: fully erased
    assert_eq!(grid.cells()[1][0].character, ' '); // cursor row: cols 0..=2 erased
    assert_eq!(grid.cells()[1][1].character, ' ');
    assert_eq!(grid.cells()[1][2].character, ' ');
    assert_eq!(grid.cells()[1][3].character, '2'); // col 3 preserved
    assert_eq!(row_text(&grid, 2), "ROW3");        // row below cursor: preserved
}

#[test]
fn ed2_erase_all() {
    // CSI 2 J — erase entire screen
    let grid = vt(b"HELLO\x1b[2;1HWORLD\x1b[2J");
    assert_eq!(row_text(&grid, 0), "");
    assert_eq!(row_text(&grid, 1), "");
}

#[test]
fn ech_erase_characters() {
    // CSI n X — erase n characters from cursor position (replace with spaces)
    let grid = vt(b"ABCDEF\x1b[1;3H\x1b[2X");
    assert_eq!(grid.cells()[0][0].character, 'A');
    assert_eq!(grid.cells()[0][1].character, 'B');
    assert_eq!(grid.cells()[0][2].character, ' ');
    assert_eq!(grid.cells()[0][3].character, ' ');
    assert_eq!(grid.cells()[0][4].character, 'E');
}

// ============================================================
// 3. SGR attributes
// ============================================================

#[test]
fn sgr_256_color_foreground() {
    // CSI 38;5;196 m — set fg to color 196 (red)
    let grid = vt(b"\x1b[38;5;196mX");
    assert_eq!(grid.cells()[0][0].fg, Color::Indexed(196));
}

#[test]
fn sgr_256_color_background() {
    // CSI 48;5;21 m — set bg to color 21 (blue)
    let grid = vt(b"\x1b[48;5;21mX");
    assert_eq!(grid.cells()[0][0].bg, Color::Indexed(21));
}

#[test]
fn sgr_rgb_foreground() {
    // CSI 38;2;255;128;0 m — set fg to RGB
    let grid = vt(b"\x1b[38;2;255;128;0mX");
    assert_eq!(grid.cells()[0][0].fg, Color::Rgb(Rgb::new(255, 128, 0)));
}

#[test]
fn sgr_rgb_background() {
    // CSI 48;2;0;255;128 m — set bg to RGB
    let grid = vt(b"\x1b[48;2;0;255;128mX");
    assert_eq!(grid.cells()[0][0].bg, Color::Rgb(Rgb::new(0, 255, 128)));
}

#[test]
fn sgr_combined_attributes() {
    // CSI 1;3;4;9 m — bold + italic + underline + strikethrough
    let grid = vt(b"\x1b[1;3;4;9mX");
    let flags = grid.cells()[0][0].flags;
    assert!(flags.contains(CellFlags::BOLD));
    assert!(flags.contains(CellFlags::ITALIC));
    assert!(flags.contains(CellFlags::UNDERLINE));
    assert!(flags.contains(CellFlags::STRIKETHROUGH));
}

#[test]
fn sgr_reset_individual_attributes() {
    // Set bold+italic, then reset italic only (CSI 23m), bold should remain
    let grid = vt(b"\x1b[1;3mA\x1b[23mB");
    assert!(grid.cells()[0][0].flags.contains(CellFlags::BOLD | CellFlags::ITALIC));
    assert!(grid.cells()[0][1].flags.contains(CellFlags::BOLD));
    assert!(!grid.cells()[0][1].flags.contains(CellFlags::ITALIC));
}

#[test]
fn sgr_default_fg_bg() {
    // CSI 39m = default fg, CSI 49m = default bg
    let grid = vt(b"\x1b[31;42mA\x1b[39mB\x1b[49mC");
    assert_eq!(grid.cells()[0][0].fg, Color::Indexed(1));
    assert_eq!(grid.cells()[0][0].bg, Color::Indexed(2));
    assert_eq!(grid.cells()[0][1].fg, Color::Default);
    assert_eq!(grid.cells()[0][1].bg, Color::Indexed(2));
    assert_eq!(grid.cells()[0][2].fg, Color::Default);
    assert_eq!(grid.cells()[0][2].bg, Color::Default);
}

// ============================================================
// 4. Line operations (IL, DL, scroll regions)
// ============================================================

#[test]
fn insert_lines() {
    // CSI n L — insert n blank lines at cursor, pushing content down
    let grid = vt(b"\x1b[1;1HAAA\x1b[2;1HBBB\x1b[3;1HCCC\x1b[2;1H\x1b[1L");
    assert_eq!(row_text(&grid, 0), "AAA");
    assert_eq!(row_text(&grid, 1), "");
    assert_eq!(row_text(&grid, 2), "BBB");
}

#[test]
fn delete_lines() {
    // CSI n M — delete n lines at cursor, pulling content up
    let grid = vt(b"\x1b[1;1HAAA\x1b[2;1HBBB\x1b[3;1HCCC\x1b[2;1H\x1b[1M");
    assert_eq!(row_text(&grid, 0), "AAA");
    assert_eq!(row_text(&grid, 1), "CCC");
    assert_eq!(row_text(&grid, 2), "");
}

#[test]
fn scroll_region_basic() {
    // DECSTBM: CSI top;bottom r — set scroll region
    // Then newline at bottom of region should scroll only within region
    let input = b"\x1b[1;1HL1\x1b[2;1HL2\x1b[3;1HL3\x1b[4;1HL4\x1b[5;1HL5\x1b[2;4r\x1b[4;1H\n";
    let grid = vt_sized(input, 80, 5);
    assert_eq!(row_text(&grid, 0), "L1"); // outside region, unchanged
    assert_eq!(row_text(&grid, 1), "L3"); // L2 scrolled out, L3 moved up
    assert_eq!(row_text(&grid, 2), "L4"); // L4 moved up
    assert_eq!(row_text(&grid, 4), "L5"); // outside region, unchanged
}

// ============================================================
// 5. Character insertion/deletion (ICH, DCH)
// ============================================================

#[test]
fn insert_characters() {
    // CSI n @ — insert n blank characters, shifting existing chars right
    let grid = vt(b"ABCD\x1b[1;2H\x1b[2@");
    assert_eq!(grid.cells()[0][0].character, 'A');
    assert_eq!(grid.cells()[0][1].character, ' ');
    assert_eq!(grid.cells()[0][2].character, ' ');
    assert_eq!(grid.cells()[0][3].character, 'B');
}

#[test]
fn delete_characters() {
    // CSI n P — delete n characters, shifting remaining chars left
    let grid = vt(b"ABCDEF\x1b[1;2H\x1b[2P");
    assert_eq!(grid.cells()[0][0].character, 'A');
    assert_eq!(grid.cells()[0][1].character, 'D');
    assert_eq!(grid.cells()[0][2].character, 'E');
}

// ============================================================
// 6. Save/Restore cursor (DECSC/DECRC)
// ============================================================

#[test]
fn save_restore_cursor_position() {
    // ESC 7 = save, ESC 8 = restore (also CSI s / CSI u)
    let grid = vt(b"\x1b[5;10H\x1b7\x1b[1;1Hxx\x1b8Y");
    assert_eq!(grid.cells()[4][9].character, 'Y');
}

// ============================================================
// 7. Reverse Index (RI)
// ============================================================

#[test]
fn reverse_index_scrolls_down_at_top() {
    // ESC M at top of screen/region should scroll content down and insert blank line at top
    let grid = vt_sized(b"\x1b[1;1HL1\x1b[2;1HL2\x1b[1;1H\x1bM", 80, 5);
    assert_eq!(row_text(&grid, 0), "");
    assert_eq!(row_text(&grid, 1), "L1");
    assert_eq!(row_text(&grid, 2), "L2");
}

#[test]
fn reverse_index_moves_up_if_not_at_top() {
    // ESC M not at top just moves cursor up one line
    let grid = vt(b"\x1b[3;5H\x1bMX");
    assert_eq!(grid.cells()[1][4].character, 'X');
}

// ============================================================
// 8. Tab stops
// ============================================================

#[test]
fn default_tab_stops_every_8_columns() {
    // xterm default: tab stops at columns 9, 17, 25, ... (1-based)
    let grid = vt(b"\tX");
    assert_eq!(grid.cells()[0][8].character, 'X');
}

// ============================================================
// 9. Line wrapping
// ============================================================

#[test]
fn auto_wrap_at_right_margin() {
    // When cursor reaches right margin, next character wraps to next line
    let mut input = vec![b'A'; 80];
    input.push(b'B');
    let grid = vt(&input);
    assert_eq!(grid.cells()[0][79].character, 'A');
    assert_eq!(grid.cells()[1][0].character, 'B');
}

// ============================================================
// 10. Alternate screen buffer
// ============================================================

#[test]
fn alt_screen_preserves_main_content() {
    // CSI ?1049h enters alt screen, CSI ?1049l leaves and restores main
    let grid = vt(b"MAIN\x1b[?1049h\x1b[1;1HALT\x1b[?1049l");
    assert_eq!(row_text(&grid, 0), "MAIN");
}

// ============================================================
// 11. Wide characters (CJK)
// ============================================================

#[test]
fn wide_char_occupies_two_columns() {
    let grid = vt("A가B".as_bytes());
    assert_eq!(grid.cells()[0][0].character, 'A');
    assert_eq!(grid.cells()[0][1].character, '가');
    assert!(grid.cells()[0][1].flags.contains(CellFlags::WIDE_CHAR));
    assert_eq!(grid.cells()[0][3].character, 'B');
}

#[test]
fn overwrite_wide_char_clears_both_cells() {
    // Writing a narrow char over the first cell of a wide char should clear the second cell
    let grid = vt("가\x1b[1;1HX".as_bytes());
    assert_eq!(grid.cells()[0][0].character, 'X');
    assert_eq!(grid.cells()[0][1].character, ' ');
}

// ============================================================
// 12. DIM attribute (SGR 2 / SGR 22)
// ============================================================

#[test]
fn sgr_dim_and_reset() {
    let grid = vt(b"\x1b[2mA\x1b[22mB");
    assert!(grid.cells()[0][0].flags.contains(CellFlags::DIM));
    assert!(!grid.cells()[0][1].flags.contains(CellFlags::DIM));
}

// ============================================================
// 13. Hidden attribute (SGR 8 / SGR 28)
// ============================================================

#[test]
fn sgr_hidden_and_reset() {
    let grid = vt(b"\x1b[8mA\x1b[28mB");
    assert!(grid.cells()[0][0].flags.contains(CellFlags::HIDDEN));
    assert!(!grid.cells()[0][1].flags.contains(CellFlags::HIDDEN));
}
