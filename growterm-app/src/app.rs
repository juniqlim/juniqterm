use std::io::Write;
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use growterm_gpu_draw::GpuDrawer;
use growterm_macos::{AppEvent, MacWindow, Modifiers};

use crate::config::CopyModeAction;
use crate::copy_mode::CopyMode;
use crate::ink_workaround::InkImeState;
use crate::pomodoro::{Pomodoro, TickResult};
use crate::selection::{self, Selection};
use crate::tab::{Tab, TabManager};
use crate::url;
use crate::zoom;

/// Copy text to system clipboard.
fn copy_to_clipboard(text: &str) {
    if !text.is_empty() {
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text(text);
        }
    }
}

/// Send SGR mouse report to PTY. `suffix` is 'M' for press/motion, 'm' for release.
fn send_sgr_mouse(tab: &mut Tab, x: f64, y: f64, y_offset: f32, cw: f32, ch: f32, button: u32, suffix: char) {
    let (row, col) = selection::pixel_to_cell(x as f32, y as f32 - y_offset, cw, ch);
    let seq = format!("\x1b[<{button};{};{}{suffix}", col as u32 + 1, row as u32 + 1);
    let _ = tab.pty_writer.write_all(seq.as_bytes());
    let _ = tab.pty_writer.flush();
}

/// Apply scrollbar drag: compute scroll offset from mouse Y position.
fn apply_scrollbar_drag(tabs: &TabManager, y: f64, screen_h: f32, tab_bar_offset: f32) {
    let content_h = screen_h - tab_bar_offset;
    let ratio = ((y as f32) - tab_bar_offset).clamp(0.0, content_h) / content_h;
    if let Some(tab) = tabs.active_tab() {
        let mut state = tab.terminal.lock().unwrap();
        let scrollback_len = state.grid.scrollback_len();
        let rows = state.grid.cells().len();
        let total = scrollback_len + rows;
        let target_top_row = (ratio * total as f32) as usize;
        let offset = scrollback_len.saturating_sub(target_top_row).min(scrollback_len);
        state.grid.set_scroll_offset(offset);
    }
}

/// Resize all tabs to the given grid dimensions.
fn resize_all_tabs(tabs: &mut TabManager, cols: u16, rows: u16) {
    for tab in tabs.tabs_mut() {
        let mut state = tab.terminal.lock().unwrap();
        state.grid.resize(cols, rows);
        drop(state);
        let _ = tab.pty_writer.resize(rows, cols);
    }
}

/// Reset state when switching tabs: exit copy mode, clear selection & preedit.
fn on_tab_switch(copy_mode: &mut CopyMode, sel: &mut Selection, preedit: &mut String, window: &MacWindow) {
    if copy_mode.active {
        copy_mode.exit(sel);
        window.set_copy_mode(false);
    }
    sel.clear();
    preedit.clear();
    window.discard_marked_text();
}

/// Exit copy mode: clear selection, reset scroll, update window state.
fn exit_copy_mode(copy_mode: &mut CopyMode, sel: &mut Selection, window: &MacWindow, tabs: &TabManager) {
    copy_mode.exit(sel);
    window.set_copy_mode(false);
    if let Some(tab) = tabs.active_tab() {
        let mut state = tab.terminal.lock().unwrap();
        state.grid.set_scroll_offset(0);
    }
}

/// Convert screen row to absolute row (including scrollback).
fn screen_to_abs_row(tabs: &TabManager, screen_row: u16) -> u32 {
    if let Some(tab) = tabs.active_tab() {
        let state = tab.terminal.lock().unwrap();
        let base = state.grid.scrollback_len().saturating_sub(state.grid.scroll_offset());
        screen_row as u32 + base as u32
    } else {
        screen_row as u32
    }
}

pub fn run(window: Arc<MacWindow>, rx: mpsc::Receiver<AppEvent>, mut drawer: GpuDrawer, mut config: crate::config::Config) {
    let (cell_w, cell_h) = drawer.cell_size();
    let mut font_size = config.font_size;
    let (width, height) = window.inner_size();

    let (cols, rows) = zoom::calc_grid_size(width, height, cell_w, cell_h);

    let mut tabs = TabManager::new();

    // Spawn initial tab (no tab bar for single tab)
    match Tab::spawn(rows, cols, window.clone()) {
        Ok(tab) => {
            tabs.add_tab(tab);
        }
        Err(e) => {
            eprintln!("Failed to spawn PTY: {e}");
            return;
        }
    }

    // Periodic 1-second redraw for pomodoro timer display
    {
        let w = window.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                w.request_redraw();
            }
        });
    }

    let mut preedit = String::new();
    let mut prev_preedit = String::new();
    let mut sel = Selection::default();
    let mut scroll_accum: f64 = 0.0;
    let mut deferred: Option<AppEvent> = None;
    let grid_dump_path = std::env::var("GROWTERM_GRID_DUMP").ok();
    let test_input = std::env::var("GROWTERM_TEST_INPUT").ok();
    let test_dropped_path = std::env::var("GROWTERM_TEST_DROPPED_PATH").ok();
    let mut test_input_sent = false;
    let mut test_drop_sent = false;
    let mut ink_state = InkImeState::new();
    let mut response_timer_enabled = config.response_timer;
    if response_timer_enabled {
        if let Some(tab) = tabs.active_tab_mut() {
            tab.response_timer.set_enabled(true);
        }
        window.set_response_timer_checked(true);
    }
    let mut copy_mode = CopyMode::new();
    let mut copy_mode_action_map = config.copy_mode_keys.build_action_map();
    let mut pomodoro = Pomodoro::new(config.pomodoro_work_minutes * 60, config.pomodoro_break_minutes * 60);
    if config.pomodoro {
        pomodoro.toggle();
        window.set_pomodoro_checked(true);
    }
    let mut coaching_enabled = config.coaching;
    window.set_coaching_checked(coaching_enabled);
    window.set_coaching_menu_enabled(config.pomodoro);
    let mut transparent_tab_bar = config.transparent_tab_bar;
    let mut header_opacity = config.header_opacity;
    window.set_transparent_tab_bar_checked(transparent_tab_bar);
    window.set_transparent_mode(transparent_tab_bar);
    let title_bar_height = if transparent_tab_bar {
        window.title_bar_height() as f32
    } else {
        0.0
    };
    let mut title_bar_height = title_bar_height;
    // hover_url_range: (abs_row, start_col, end_col) for Cmd+hover URL underline
    let mut hover_url_range: Option<(u32, u16, u16)> = None;
    let mut scrollbar_dragging = false;
    let mut scrollbar_visible_until: Option<Instant> = None;
    const SCROLLBAR_HIT_WIDTH: f32 = 20.0;
    const SCROLLBAR_SHOW_DURATION: Duration = Duration::from_millis(1500);
    // copy flash: screen row to highlight briefly after Cmd+A
    let mut copy_flash: Option<(u16, u16, Instant)> = None;
    const COPY_FLASH_DURATION: Duration = Duration::from_millis(150);
    let mut tab_dragging: Option<usize> = None;
    let mut tab_drag_start_x: f32 = 0.0;

    macro_rules! do_render {
        () => {
            render_with_tabs(&mut drawer, &tabs, &preedit, &sel, &ink_state, hover_url_range, pomodoro.is_input_blocked(), pomodoro.coaching_lines().as_deref(), scrollbar_dragging || scrollbar_visible_until.map_or(false, |t| t > Instant::now()), copy_flash, tab_dragging, transparent_tab_bar, title_bar_height, header_opacity)
        };
        (scrollbar: true) => {
            render_with_tabs(&mut drawer, &tabs, &preedit, &sel, &ink_state, hover_url_range, pomodoro.is_input_blocked(), pomodoro.coaching_lines().as_deref(), true, copy_flash, tab_dragging, transparent_tab_bar, title_bar_height, header_opacity)
        };
    }

    loop {
        let event = if let Some(evt) = deferred.take() {
            evt
        } else {
            match rx.recv() {
                Ok(evt) => evt,
                Err(_) => break,
            }
        };
        let has_scrollback = tabs.active_tab()
            .map(|t| t.terminal.lock().unwrap().grid.scrollback_len() > 0)
            .unwrap_or(false);
        match event {
            AppEvent::TextCommit(text) => {
                preedit.clear();
                // 백틱(`) 또는 ₩: 복사모드 진입/종료
                if (text == "`" || text == "₩") && !copy_mode.active {
                    if let Some(tab) = tabs.active_tab() {
                        let state = tab.terminal.lock().unwrap();
                        let sb_len = state.grid.scrollback_len() as u32;
                        let (cursor_row, _cursor_col) = state.grid.cursor_pos();
                        let abs_cursor_row = sb_len + cursor_row as u32;
                        let cols = state.grid.cells().first().map_or(80, |r| r.len()) as u16;
                        drop(state);
                        copy_mode.enter(abs_cursor_row, cols, &mut sel);
                        window.set_copy_mode(true);
                        window.discard_marked_text();
                    }
                    do_render!();
                    continue;
                }
                ink_state.on_text_commit(&text);
                if pomodoro.is_input_blocked() {
                    continue;
                }
                pomodoro.on_input(&tab_scrollback_lens(&tabs));
                if let Some(tab) = tabs.active_tab_mut() {
                    let _ = tab.pty_writer.write_all(text.as_bytes());
                    let _ = tab.pty_writer.flush();
                }
            }
            AppEvent::Preedit(text) => {
                if !text.is_empty() {
                    let child_pid = tabs.active_tab()
                        .and_then(|t| t.pty_writer.child_pid());
                    ink_state.on_preedit(child_pid);
                }
                preedit = text;
                window.request_redraw();
            }
            AppEvent::KeyInput {
                keycode,
                characters,
                modifiers,
            } => {
                use growterm_macos::key_convert::keycode as kc;

                if modifiers.contains(Modifiers::SUPER) {
                    // Cmd+N: new window (spawn new process)
                    if keycode == kc::ANSI_N {
                        spawn_new_window();
                        continue;
                    }

                    // Cmd+T: new tab (inherit CWD from active tab)
                    if keycode == kc::ANSI_T {
                        let (cw, ch) = drawer.cell_size();
                        let (w, h) = window.inner_size();
                        let (cols, _rows) = zoom::calc_grid_size(w, h, cw, ch);
                        let had_no_tab_bar = !tabs.show_tab_bar();
                        // After adding a tab, tab bar will show — compute rows with tab bar
                        let term_rows = ((h as f32 - drawer.tab_bar_height()) / ch).floor().max(1.0) as u16;
                        let active_cwd = tabs
                            .active_tab()
                            .and_then(|t| t.pty_writer.child_pid())
                            .and_then(growterm_pty::child_cwd);
                        match Tab::spawn_with_cwd(term_rows, cols, window.clone(), active_cwd.as_deref()) {
                            Ok(mut tab) => {
                                tab.response_timer.set_enabled(response_timer_enabled);
                                tabs.add_tab(tab);
                                if copy_mode.active {
                                    copy_mode.exit(&mut sel);
                                    window.set_copy_mode(false);
                                }
                                sel.clear();
                                preedit.clear();
                                window.discard_marked_text();
                                // Tab bar just appeared — shrink existing tabs by 1 row
                                if had_no_tab_bar && tabs.show_tab_bar() {
                                    for t in tabs.tabs_mut() {
                                        let mut st = t.terminal.lock().unwrap();
                                        st.grid.resize(cols, term_rows);
                                        drop(st);
                                        let _ = t.pty_writer.resize(term_rows, cols);
                                    }
                                }
                            }
                            Err(e) => eprintln!("Failed to spawn tab: {e}"),
                        }
                        do_render!();
                        continue;
                    }

                    // Cmd+W: close tab
                    if keycode == kc::ANSI_W {
                        let had_tab_bar = tabs.show_tab_bar();
                        tabs.close_active();
                        if tabs.is_empty() {
                            std::process::exit(0);
                        }
                        if copy_mode.active {
                            copy_mode.exit(&mut sel);
                            window.set_copy_mode(false);
                        }
                        sel.clear();
                        preedit.clear();
                        // Tab bar just disappeared — expand remaining tab by 1 row
                        if had_tab_bar && !tabs.show_tab_bar() {
                            let (cw, ch) = drawer.cell_size();
                            let (w, h) = window.inner_size();
                            let (cols, rows) = zoom::calc_grid_size(w, h, cw, ch);
                            if let Some(t) = tabs.active_tab_mut() {
                                let mut st = t.terminal.lock().unwrap();
                                st.grid.resize(cols, rows);
                                drop(st);
                                let _ = t.pty_writer.resize(rows, cols);
                            }
                        }
                        do_render!();
                        continue;
                    }

                    // Cmd+Shift+[ / Cmd+Shift+]: prev/next tab
                    if modifiers.contains(Modifiers::SHIFT) {
                        // Cmd+Shift+R: reload config — 메뉴(reloadConfig:)로 처리됨
                        if keycode == kc::ANSI_LEFT_BRACKET {
                            tabs.prev_tab();
                            on_tab_switch(&mut copy_mode, &mut sel, &mut preedit, &window);
                            do_render!();
                            continue;
                        }
                        if keycode == kc::ANSI_RIGHT_BRACKET {
                            tabs.next_tab();
                            on_tab_switch(&mut copy_mode, &mut sel, &mut preedit, &window);
                            do_render!();
                            continue;
                        }
                    }

                    // Cmd+1~9: switch to tab by number
                    let tab_num = match keycode {
                        k if k == kc::ANSI_1 => Some(0),
                        k if k == kc::ANSI_2 => Some(1),
                        k if k == kc::ANSI_3 => Some(2),
                        k if k == kc::ANSI_4 => Some(3),
                        k if k == kc::ANSI_5 => Some(4),
                        k if k == kc::ANSI_6 => Some(5),
                        k if k == kc::ANSI_7 => Some(6),
                        k if k == kc::ANSI_8 => Some(7),
                        k if k == kc::ANSI_9 => Some(8),
                        _ => None,
                    };
                    if let Some(idx) = tab_num {
                        if idx < tabs.tab_count() {
                            tabs.switch_to(idx);
                            on_tab_switch(&mut copy_mode, &mut sel, &mut preedit, &window);
                            do_render!();
                        }
                        continue;
                    }

                    // Cmd+PageUp/Down: scroll one page
                    if keycode == kc::PAGE_UP || keycode == kc::PAGE_DOWN {
                        if let Some(tab) = tabs.active_tab() {
                            let mut state = tab.terminal.lock().unwrap();
                            let row_count = state.grid.cells().len();
                            if keycode == kc::PAGE_UP {
                                state.grid.scroll_up_view(row_count);
                            } else {
                                state.grid.scroll_down_view(row_count);
                            }
                        }
                        scrollbar_visible_until = Some(Instant::now() + SCROLLBAR_SHOW_DURATION);
                        do_render!(scrollbar: true);
                        continue;
                    }

                    // Cmd+Home: scroll to top, Cmd+End: scroll to bottom
                    if keycode == kc::HOME || keycode == kc::END {
                        if let Some(tab) = tabs.active_tab() {
                            let mut state = tab.terminal.lock().unwrap();
                            if keycode == kc::HOME {
                                let max = state.grid.scrollback_len();
                                state.grid.scroll_up_view(max);
                            } else {
                                state.grid.reset_scroll();
                            }
                        }
                        scrollbar_visible_until = Some(Instant::now() + SCROLLBAR_SHOW_DURATION);
                        do_render!(scrollbar: true);
                        continue;
                    }

                    // Cmd+A: copy input line to clipboard
                    if keycode == kc::ANSI_A {
                        if let Some(tab) = tabs.active_tab() {
                            let state = tab.terminal.lock().unwrap();
                            let (text, flash_start, flash_end) = selection::input_line_text(&state.grid);
                            drop(state);
                            copy_to_clipboard(&text);
                            copy_flash = Some((flash_start, flash_end, Instant::now()));
                            do_render!();
                            let w = window.clone();
                            std::thread::spawn(move || {
                                std::thread::sleep(COPY_FLASH_DURATION);
                                w.request_redraw();
                            });
                        }
                        continue;
                    }

                    // Cmd+Shift+C: 복사모드 진입/종료 토글
                    if keycode == kc::ANSI_C && modifiers.contains(Modifiers::SHIFT) {
                        if copy_mode.active {
                            exit_copy_mode(&mut copy_mode, &mut sel, &window, &tabs);
                        } else if let Some(tab) = tabs.active_tab() {
                            let state = tab.terminal.lock().unwrap();
                            let sb_len = state.grid.scrollback_len() as u32;
                            let (cursor_row, _cursor_col) = state.grid.cursor_pos();
                            let abs_cursor_row = sb_len + cursor_row as u32;
                            let cols = state.grid.cells().first().map_or(80, |r| r.len()) as u16;
                            drop(state);
                            copy_mode.enter(abs_cursor_row, cols, &mut sel);
                            window.set_copy_mode(true);
                            window.discard_marked_text();
                        }
                        do_render!();
                        continue;
                    }

                    // Cmd+C copy
                    if keycode == kc::ANSI_C {
                        if !sel.is_empty() {
                            if let Some(tab) = tabs.active_tab() {
                                let state = tab.terminal.lock().unwrap();
                                let text = selection::extract_text_absolute(&state.grid, &sel);
                                drop(state);
                                copy_to_clipboard(&text);
                            }
                        }
                        continue;
                    }

                    // Cmd+V paste
                    if keycode == kc::ANSI_V {
                        if let Ok(mut clipboard) = arboard::Clipboard::new() {
                            if let Ok(text) = clipboard.get_text() {
                                if !text.is_empty() {
                                    if let Some(tab) = tabs.active_tab_mut() {
                                        let bp = tab.bracketed_paste.load(std::sync::atomic::Ordering::Relaxed);
                                        if bp {
                                            let _ = tab.pty_writer.write_all(b"\x1b[200~");
                                        }
                                        let _ = tab.pty_writer.write_all(text.as_bytes());
                                        if bp {
                                            let _ = tab.pty_writer.write_all(b"\x1b[201~");
                                        }
                                        let _ = tab.pty_writer.flush();
                                    }
                                }
                            }
                        }
                        continue;
                    }

                    // Cmd+= / Cmd+- (zoom)
                    let zoom_delta = match keycode {
                        k if k == kc::ANSI_EQUAL => Some(2.0f32),
                        k if k == kc::ANSI_MINUS => Some(-2.0f32),
                        _ => None,
                    };
                    if let Some(delta) = zoom_delta {
                        font_size = zoom::apply_zoom(font_size, delta);
                        drawer.set_font_size(font_size);
                        let (cw, ch) = drawer.cell_size();
                        let (w, h) = window.inner_size();
                        let (cols, _rows) = zoom::calc_grid_size(w, h, cw, ch);
                        let term_rows = tabs.term_rows(h, ch, drawer.tab_bar_height());
                        resize_all_tabs(&mut tabs, cols, term_rows);
                        do_render!();
                        continue;
                    }
                    continue;
                }

                // 복사모드: PTY 전송 건너뛰고 raw keycode로 처리
                if copy_mode.active {
                    let cols = tabs.active_tab().map_or(80u16, |t| {
                        let state = t.terminal.lock().unwrap();
                        state.grid.cells().first().map_or(80, |r| r.len()) as u16
                    });
                    let max_row = tabs.active_tab().map_or(0u32, |t| {
                        let state = t.terminal.lock().unwrap();
                        let sb_len = state.grid.scrollback_len() as u32;
                        let screen_rows = state.grid.cells().len() as u32;
                        sb_len + screen_rows - 1
                    });

                    if let Some(action) = copy_mode_action_map.get(&keycode) {
                        match action {
                            CopyModeAction::Down => {
                                copy_mode.move_down(cols, max_row, &mut sel);
                            }
                            CopyModeAction::Up => {
                                copy_mode.move_up(cols, &mut sel);
                            }
                            CopyModeAction::Visual => {
                                copy_mode.toggle_visual(cols, &mut sel);
                            }
                            CopyModeAction::HalfPageDown => {
                                copy_mode.move_right(cols, max_row, &mut sel);
                            }
                            CopyModeAction::HalfPageUp => {
                                copy_mode.move_left(cols, &mut sel);
                            }
                            CopyModeAction::Yank => {
                                // y: 선택 텍스트 클립보드에 복사 후 모드 종료
                                if !sel.is_empty() {
                                    if let Some(tab) = tabs.active_tab() {
                                        let state = tab.terminal.lock().unwrap();
                                        let text = selection::extract_text_absolute(&state.grid, &sel);
                                        drop(state);
                                        copy_to_clipboard(&text);
                                    }
                                }
                                exit_copy_mode(&mut copy_mode, &mut sel, &window, &tabs);
                            }
                            CopyModeAction::Exit => {
                                exit_copy_mode(&mut copy_mode, &mut sel, &window, &tabs);
                            }
                        }
                    }

                    // 커서 행이 화면에 보이도록 스크롤 조정 (복사모드 활성 중에만)
                    if copy_mode.active {
                    if let Some(tab) = tabs.active_tab() {
                        let mut state = tab.terminal.lock().unwrap();
                        let sb_len = state.grid.scrollback_len();
                        let visible_rows = state.grid.cells().len();
                        let offset = state.grid.scroll_offset();
                        let view_top = sb_len.saturating_sub(offset) as u32;
                        let view_bottom = view_top + visible_rows as u32;
                        let cursor_row = copy_mode.cursor.0;
                        if cursor_row < view_top {
                            let new_offset = sb_len.saturating_sub(cursor_row as usize);
                            state.grid.set_scroll_offset(new_offset);
                        } else if cursor_row >= view_bottom {
                            let new_offset = sb_len.saturating_sub(cursor_row as usize + 1 - visible_rows);
                            state.grid.set_scroll_offset(new_offset);
                        }
                    }
                    }

                    do_render!();
                    continue;
                }

                if pomodoro.is_input_blocked() {
                    continue;
                }
                if let Some(key_event) =
                    growterm_macos::convert_key(keycode, characters.as_deref(), modifiers)
                {
                    let bytes = growterm_input::encode(key_event);
                    pomodoro.on_input(&tab_scrollback_lens(&tabs));
                    if bytes == b"\r" || bytes == b"\n" {
                        ink_state.on_enter();
                        if let Some(tab) = tabs.active_tab_mut() {
                            tab.response_timer.on_enter();
                        }
                    } else {
                        ink_state.on_key_input(&bytes);
                    }
                    if let Some(tab) = tabs.active_tab_mut() {
                        let _ = tab.pty_writer.write_all(&bytes);
                        let _ = tab.pty_writer.flush();
                    }
                }
            }
            AppEvent::MouseDown(x, y, modifiers) => {
                let (cw, ch) = drawer.cell_size();

                // Tab bar click: start drag
                if tabs.show_tab_bar() && crate::tab::hit_test_tab_bar(y as f32, drawer.tab_bar_height(), tabs.tab_bar_y(title_bar_height)) {
                    let screen_w = window.inner_size().0 as f32;
                    if let Some(index) = tabs.tab_index_at_x(x as f32, screen_w) {
                        tab_dragging = Some(index);
                        tab_drag_start_x = x as f32;
                        window.request_redraw();
                    }
                    continue;
                }

                // Mouse tracking: send SGR report to PTY
                {
                    let y_offset = tabs.mouse_y_offset(drawer.tab_bar_height(), title_bar_height, has_scrollback);
                    if let Some(tab) = tabs.active_tab_mut() {
                        let mode = tab.mouse_mode.load(Ordering::Relaxed);
                        if mode > 0 {
                            send_sgr_mouse(tab, x, y, y_offset, cw, ch, 0, 'M');
                            continue;
                        }
                    }
                }

                // Scrollbar area click: start dragging
                let screen_w = window.inner_size().0 as f32;
                let screen_h = window.inner_size().1 as f32;
                if (x as f32) >= screen_w - SCROLLBAR_HIT_WIDTH {
                    let has_scrollback_content = tabs.active_tab().map_or(false, |tab| {
                        tab.terminal.lock().unwrap().grid.scrollback_len() > 0
                    });
                    if has_scrollback_content {
                        scrollbar_dragging = true;
                        scrollbar_visible_until = Some(Instant::now() + SCROLLBAR_SHOW_DURATION);
                        let tab_bar_offset = tabs.mouse_y_offset(drawer.tab_bar_height(), title_bar_height, has_scrollback);
                        apply_scrollbar_drag(&tabs, y, screen_h, tab_bar_offset);
                        do_render!(scrollbar: true);
                        continue;
                    }
                }

                let (screen_row, col) =
                    selection::mouse_pixel_to_cell(x as f32, y as f32, cw, ch, tabs.mouse_y_offset(drawer.tab_bar_height(), title_bar_height, has_scrollback));
                let abs_row = screen_to_abs_row(&tabs, screen_row);

                // Cmd+Click: open URL under cursor
                if modifiers.contains(Modifiers::SUPER) {
                    if let Some(tab) = tabs.active_tab() {
                        let state = tab.terminal.lock().unwrap();
                        let row_text = selection::row_text_absolute(&state.grid, abs_row);
                        drop(state);
                        if let Some(found_url) = url::find_url_at(&row_text, col as usize) {
                            let _ = std::process::Command::new("open")
                                .arg(found_url)
                                .spawn();
                        }
                    }
                    hover_url_range = None;
                    window.request_redraw();
                    continue;
                }

                sel.begin(abs_row, col);
                window.request_redraw();
            }
            AppEvent::MouseDragged(x, y) => {
                if let Some(drag_idx) = tab_dragging {
                    let screen_w = window.inner_size().0 as f32;
                    if let Some(target) = tabs.tab_index_at_x(x as f32, screen_w) {
                        if target != drag_idx {
                            tabs.move_tab(drag_idx, target);
                            tab_dragging = Some(target);
                            window.request_redraw();
                        }
                    }
                    continue;
                }
                // Mouse tracking: send SGR drag report to PTY
                {
                    let y_offset = tabs.mouse_y_offset(drawer.tab_bar_height(), title_bar_height, has_scrollback);
                    if let Some(tab) = tabs.active_tab_mut() {
                        let mode = tab.mouse_mode.load(Ordering::Relaxed);
                        if mode >= 2 {
                            let (cw, ch) = drawer.cell_size();
                            send_sgr_mouse(tab, x, y, y_offset, cw, ch, 32, 'M');
                            continue;
                        }
                    }
                }
                if scrollbar_dragging {
                    let screen_h = window.inner_size().1 as f32;
                    let tab_bar_offset = tabs.mouse_y_offset(drawer.tab_bar_height(), title_bar_height, has_scrollback);
                    apply_scrollbar_drag(&tabs, y, screen_h, tab_bar_offset);
                    scrollbar_visible_until = Some(Instant::now() + SCROLLBAR_SHOW_DURATION);
                    do_render!(scrollbar: true);
                } else if sel.active {
                    let (cw, ch) = drawer.cell_size();
                    let (screen_row, col) = selection::mouse_pixel_to_cell(
                        x as f32, y as f32, cw, ch,
                        tabs.mouse_y_offset(drawer.tab_bar_height(), title_bar_height, has_scrollback),
                    );
                    let abs_row = screen_to_abs_row(&tabs, screen_row);
                    sel.update(abs_row, col);
                    window.request_redraw();
                }
            }
            AppEvent::MouseUp(x, y) => {
                if let Some(drag_idx) = tab_dragging.take() {
                    let drag_distance = (x as f32 - tab_drag_start_x).abs();
                    let screen_w = window.inner_size().0 as f32;
                    let tab_w = screen_w / tabs.tab_count().max(1) as f32;
                    if drag_distance < tab_w * 0.3 {
                        // Small movement = click: switch to tab
                        tabs.switch_to(drag_idx);
                        preedit.clear();
                        window.discard_marked_text();
                    }
                    window.request_redraw();
                    continue;
                }
                if scrollbar_dragging {
                    scrollbar_dragging = false;
                    continue;
                }
                let (cw, ch) = drawer.cell_size();
                if tabs.show_tab_bar() && crate::tab::hit_test_tab_bar(y as f32, drawer.tab_bar_height(), tabs.tab_bar_y(title_bar_height)) {
                    continue;
                }

                // Mouse tracking: send SGR release report to PTY
                {
                    let y_offset = tabs.mouse_y_offset(drawer.tab_bar_height(), title_bar_height, has_scrollback);
                    if let Some(tab) = tabs.active_tab_mut() {
                        let mode = tab.mouse_mode.load(Ordering::Relaxed);
                        if mode > 0 {
                            send_sgr_mouse(tab, x, y, y_offset, cw, ch, 0, 'm');
                            continue;
                        }
                    }
                }

                let (screen_row, col) =
                    selection::mouse_pixel_to_cell(x as f32, y as f32, cw, ch, tabs.mouse_y_offset(drawer.tab_bar_height(), title_bar_height, has_scrollback));
                let abs_row = screen_to_abs_row(&tabs, screen_row);
                sel.update(abs_row, col);
                sel.finish();
                window.request_redraw();
            }
            AppEvent::MouseMoved(x, y, modifiers) => {
                let new_range = if modifiers.contains(Modifiers::SUPER) {
                    let (cw, ch) = drawer.cell_size();
                    let (screen_row, col) = selection::mouse_pixel_to_cell(
                        x as f32, y as f32, cw, ch,
                        tabs.mouse_y_offset(drawer.tab_bar_height(), title_bar_height, has_scrollback),
                    );
                    if let Some(tab) = tabs.active_tab() {
                        let abs_row = screen_to_abs_row(&tabs, screen_row);
                        let state = tab.terminal.lock().unwrap();
                        let row_text = selection::row_text_absolute(&state.grid, abs_row);
                        drop(state);
                        if let Some((start, end)) = url::find_url_range_at(&row_text, col as usize)
                        {
                            Some((abs_row, start as u16, end as u16))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };
                if new_range != hover_url_range {
                    hover_url_range = new_range;
                    window.request_redraw();
                }
            }
            AppEvent::ScrollWheel(delta_y) => {
                // Mouse tracking: send SGR scroll report to PTY
                if let Some(tab) = tabs.active_tab_mut() {
                    let mode = tab.mouse_mode.load(Ordering::Relaxed);
                    if mode > 0 {
                        scroll_accum += delta_y;
                        let (_, ch) = drawer.cell_size();
                        let line_height = if ch > 0.0 { ch as f64 } else { 20.0 };
                        let lines = (scroll_accum / line_height).trunc() as i32;
                        if lines != 0 {
                            scroll_accum -= lines as f64 * line_height;
                            // btn 64=scroll up, 65=scroll down
                            let btn = if lines > 0 { 64 } else { 65 };
                            let count = lines.unsigned_abs() as usize;
                            // Use col=1, row=1 as default position for scroll events
                            for _ in 0..count {
                                let seq = format!("\x1b[<{btn};1;1M");
                                let _ = tab.pty_writer.write_all(seq.as_bytes());
                            }
                            let _ = tab.pty_writer.flush();
                        }
                        continue;
                    }
                }
                scroll_accum += delta_y;
                let (_, ch) = drawer.cell_size();
                let line_height = if ch > 0.0 { ch as f64 } else { 20.0 };
                let lines = (scroll_accum / line_height).trunc() as i32;
                if lines != 0 {
                    scroll_accum -= lines as f64 * line_height;
                    if let Some(tab) = tabs.active_tab() {
                        let mut state = tab.terminal.lock().unwrap();
                        if lines > 0 {
                            state.grid.scroll_up_view(lines as usize);
                        } else {
                            state.grid.scroll_down_view((-lines) as usize);
                        }
                    }
                    scrollbar_visible_until = Some(Instant::now() + SCROLLBAR_SHOW_DURATION);
                    do_render!(scrollbar: true);
                }
            }
            AppEvent::Resize(mut w, mut h) => {
                loop {
                    match rx.try_recv() {
                        Ok(AppEvent::Resize(nw, nh)) => {
                            w = nw;
                            h = nh;
                        }
                        Ok(other) => {
                            deferred = Some(other);
                            break;
                        }
                        Err(_) => break,
                    }
                }
                drawer.resize(w, h);
                let (cw, ch) = drawer.cell_size();
                let (cols, _rows) = zoom::calc_grid_size(w, h, cw, ch);
                let term_rows = tabs.term_rows(h, ch, drawer.tab_bar_height());
                resize_all_tabs(&mut tabs, cols, term_rows);
                do_render!();
            }
            AppEvent::RedrawRequested => {
                // Expire copy flash
                if let Some((_, _, t)) = copy_flash {
                    if t.elapsed() >= COPY_FLASH_DURATION {
                        copy_flash = None;
                    }
                }
                if pomodoro.tick() == TickResult::StartedBreak {
                    if coaching_enabled {
                        let tab_text = extract_pomodoro_tab_text(&tabs, &pomodoro);
                        if !tab_text.trim().is_empty() {
                            let ai_handle = pomodoro.ai_response_handle();
                            crate::pomodoro::spawn_ai_coaching(tab_text, ai_handle, config.coaching_command.clone());
                        } else {
                            pomodoro.set_ai_response(vec!["작업 내용을 캡처하지 못했습니다.".to_string()]);
                        }
                    }
                }
                // Feed PTY output timestamp to each tab's response timer
                for tab in tabs.tabs_mut() {
                    let ts = tab.last_pty_output_at.lock().unwrap().take();
                    if let Some(ts) = ts {
                        tab.response_timer.on_pty_output(ts);
                    }
                    tab.response_timer.tick();
                }
                // Skip rendering while the PTY app is inside a synchronized
                // output block to avoid painting an intermediate state.
                let in_sync = tabs
                    .active_tab()
                    .map_or(false, |t| t.sync_output.load(Ordering::Relaxed));
                if in_sync {
                    continue;
                }
                let was_dirty = tabs
                    .active_tab()
                    .map_or(false, |t| t.dirty.swap(false, Ordering::Relaxed));
                let preedit_changed = preedit != prev_preedit;
                if preedit_changed {
                    prev_preedit = preedit.clone();
                }
                // Update window title with pomodoro + global avg
                let title = build_title(&pomodoro, &tabs);
                window.set_title(&title);
                do_render!();
                if was_dirty || preedit_changed {
                    if let Some(ref path) = grid_dump_path {
                        let dump_file = std::path::Path::new(path);
                        if dump_file.exists() {
                            continue;
                        }
                        if let Some(tab) = tabs.active_tab_mut() {
                            let state = tab.terminal.lock().unwrap();
                            let has_content =
                                state
                                    .grid
                                    .cells()
                                    .iter()
                                    .any(|row: &Vec<growterm_types::Cell>| {
                                        row.iter()
                                            .any(|c| c.character != '\0' && c.character != ' ')
                                    });
                            if has_content {
                                let (crow, ccol) = state.grid.cursor_pos();
                                let mut dump = format!("cursor:{crow},{ccol}\ngrid:\n");
                                for (row_idx, row) in state.grid.cells().iter().enumerate() {
                                    let mut text: String = row
                                        .iter()
                                        .map(|c: &growterm_types::Cell| c.character)
                                        .collect();
                                    // Overlay preedit at cursor position
                                    if !preedit.is_empty() && row_idx == crow as usize {
                                        let col = ccol as usize;
                                        let mut chars: Vec<char> = text.chars().collect();
                                        for (j, pc) in preedit.chars().enumerate() {
                                            let pos = col + j;
                                            while chars.len() <= pos {
                                                chars.push(' ');
                                            }
                                            chars[pos] = pc;
                                        }
                                        text = chars.into_iter().collect();
                                    }
                                    dump.push_str(
                                        text.trim_end_matches(|c: char| c == '\0' || c == ' '),
                                    );
                                    dump.push('\n');
                                }
                                drop(state);
                                if let Some(ref dropped_path) = test_dropped_path {
                                    if !test_drop_sent && !dropped_path.is_empty() {
                                        test_drop_sent = true;
                                        deferred =
                                            Some(AppEvent::FileDropped(vec![dropped_path.clone()]));
                                        continue;
                                    }
                                }
                                if let Some(ref input) = test_input {
                                    if !test_input_sent {
                                        let _ = tab.pty_writer.write_all(input.as_bytes());
                                        let _ = tab.pty_writer.flush();
                                        test_input_sent = true;
                                        continue;
                                    }
                                }
                                let _ = std::fs::write(path, &dump);
                            }
                        }
                    }
                }
            }
            AppEvent::FileDropped(paths) => {
                if let Some(tab) = tabs.active_tab_mut() {
                    let text = paths
                        .iter()
                        .map(|p| shell_escape(p))
                        .collect::<Vec<_>>()
                        .join(" ");
                    let _ = tab.pty_writer.write_all(text.as_bytes());
                    let _ = tab.pty_writer.flush();
                }
            }
            AppEvent::TogglePomodoro => {
                pomodoro.toggle();
                let enabled = pomodoro.is_enabled();
                config.pomodoro = enabled;
                window.set_pomodoro_checked(enabled);
                window.set_coaching_menu_enabled(enabled);
                if !enabled {
                    coaching_enabled = false;
                    config.coaching = false;
                    window.set_coaching_checked(false);
                }
                config.save();
                let title = build_title(&pomodoro, &tabs);
                window.set_title(&title);
            }
            AppEvent::ToggleResponseTimer => {
                response_timer_enabled = !response_timer_enabled;
                for tab in tabs.tabs_mut() {
                    tab.response_timer.set_enabled(response_timer_enabled);
                }
                config.response_timer = response_timer_enabled;
                config.save();
                window.set_response_timer_checked(response_timer_enabled);
                let title = build_title(&pomodoro, &tabs);
                window.set_title(&title);
            }
            AppEvent::ToggleCoaching => {
                coaching_enabled = !coaching_enabled;
                config.coaching = coaching_enabled;
                config.save();
                window.set_coaching_checked(coaching_enabled);
            }
            AppEvent::ToggleTransparentTabBar => {
                transparent_tab_bar = !transparent_tab_bar;
                config.transparent_tab_bar = transparent_tab_bar;
                config.save();
                window.set_transparent_tab_bar_checked(transparent_tab_bar);
                window.set_transparent_mode(transparent_tab_bar);
                title_bar_height = if transparent_tab_bar {
                    window.title_bar_height() as f32
                } else {
                    0.0
                };
            }
            AppEvent::ReloadConfig => {
                let new_config = crate::config::Config::load();
                // Apply font changes
                if new_config.font_family != config.font_family || new_config.font_size != config.font_size {
                    font_size = new_config.font_size;
                    let font_path = crate::resolve_font_path(&new_config.font_family);
                    drawer.set_font(font_path.as_deref(), font_size);
                    let (cw, ch) = drawer.cell_size();
                    let (w, h) = window.inner_size();
                    let (cols, _rows) = zoom::calc_grid_size(w, h, cw, ch);
                    let term_rows = tabs.term_rows(h, ch, drawer.tab_bar_height());
                    resize_all_tabs(&mut tabs, cols, term_rows);
                }
                // Apply pomodoro time changes
                if new_config.pomodoro_work_minutes != config.pomodoro_work_minutes
                    || new_config.pomodoro_break_minutes != config.pomodoro_break_minutes
                {
                    let was_enabled = pomodoro.is_enabled();
                    pomodoro = Pomodoro::new(
                        new_config.pomodoro_work_minutes * 60,
                        new_config.pomodoro_break_minutes * 60,
                    );
                    if was_enabled {
                        pomodoro.toggle();
                    }
                }
                // Apply toggle changes
                if new_config.pomodoro != config.pomodoro {
                    pomodoro.toggle();
                    window.set_pomodoro_checked(new_config.pomodoro);
                    window.set_coaching_menu_enabled(new_config.pomodoro);
                }
                if new_config.response_timer != config.response_timer {
                    response_timer_enabled = new_config.response_timer;
                    for tab in tabs.tabs_mut() {
                        tab.response_timer.set_enabled(response_timer_enabled);
                    }
                    window.set_response_timer_checked(response_timer_enabled);
                }
                if new_config.coaching != config.coaching {
                    coaching_enabled = new_config.coaching;
                    window.set_coaching_checked(coaching_enabled);
                }
                if new_config.transparent_tab_bar != config.transparent_tab_bar {
                    transparent_tab_bar = new_config.transparent_tab_bar;
                    window.set_transparent_tab_bar_checked(transparent_tab_bar);
                    window.set_transparent_mode(transparent_tab_bar);
                    title_bar_height = if transparent_tab_bar {
                        window.title_bar_height() as f32
                    } else {
                        0.0
                    };
                }
                header_opacity = new_config.header_opacity;
                copy_mode_action_map = new_config.copy_mode_keys.build_action_map();
                config = new_config;
            }
            AppEvent::CloseRequested => {
                std::process::exit(0);
            }
        }
    }
}


fn spawn_new_window() {
    let Ok(exe) = std::env::current_exe() else { return };
    let exe = exe.canonicalize().unwrap_or(exe);
    let exe_str = exe.to_string_lossy();

    if let Some(idx) = exe_str.find(".app/") {
        // Inside .app bundle — use `open -n` to launch a new instance
        let app_path = &exe_str[..idx + 4]; // include ".app"
        let _ = std::process::Command::new("open")
            .args(["-n", app_path])
            .spawn();
    } else {
        // Dev environment — run binary directly
        let _ = std::process::Command::new(exe).spawn();
    }
}

/// Collect (tab_id, scrollback_len + cursor_row + 1) for all tabs.
/// This captures the absolute row just past the cursor, so only new output
/// after this point will be included in coaching text.
fn tab_scrollback_lens(tabs: &TabManager) -> Vec<(u64, usize)> {
    tabs.tabs()
        .iter()
        .map(|tab| {
            let state = tab.terminal.lock().unwrap();
            let (cursor_row, _) = state.grid.cursor_pos();
            let abs = state.grid.scrollback_len() + cursor_row as usize + 1;
            (tab.id, abs)
        })
        .collect()
}

/// Extract text from a grid starting from `start_abs` row.
/// Returns the text of all rows from `start_abs` to the end.
pub fn extract_grid_text(grid: &growterm_grid::Grid, start_abs: usize) -> String {
    let current_total = grid.scrollback_len() + grid.cells().len();
    if current_total <= start_abs {
        return String::new();
    }
    let mut result = String::new();
    for abs_row in start_abs..current_total {
        let text = selection::row_text_absolute(grid, abs_row as u32);
        let trimmed = text.trim_end();
        if !trimmed.is_empty() {
            result.push_str(trimmed);
        }
        result.push('\n');
    }
    result
}

/// Extract terminal text from each tab since the pomodoro work phase started.
fn extract_pomodoro_tab_text(tabs: &TabManager, pomodoro: &Pomodoro) -> String {
    let snapshot = pomodoro.scrollback_snapshot();
    let mut result = String::new();
    for (i, tab) in tabs.tabs().iter().enumerate() {
        let start_abs = match snapshot.get(&tab.id) {
            Some(&v) => v,
            None => continue,
        };
        let state = tab.terminal.lock().unwrap();
        let text = extract_grid_text(&state.grid, start_abs);
        if text.trim().is_empty() {
            continue;
        }
        if !result.is_empty() {
            result.push_str("\n\n");
        }
        result.push_str(&format!("[Tab {}]\n", i + 1));
        result.push_str(&text);
    }
    result
}

fn build_title(pomodoro: &Pomodoro, tabs: &TabManager) -> String {
    use std::time::Duration;
    let mut total_sum = Duration::ZERO;
    let mut total_count = 0u32;
    let mut any_enabled = false;
    for tab in tabs.tabs() {
        if tab.response_timer.is_enabled() {
            any_enabled = true;
            let (sum, count) = tab.response_timer.stats();
            total_sum += sum;
            total_count += count;
        }
    }
    let avg_text = if any_enabled && total_count > 0 {
        Some(format!("{}s/{}", (total_sum / total_count).as_secs(), total_count))
    } else {
        None
    };
    match (pomodoro.display_text(), avg_text) {
        (Some(p), Some(a)) => format!("{p} | {a}"),
        (Some(p), None) => p,
        (None, Some(a)) => a,
        (None, None) => "growTerm".to_string(),
    }
}


fn shell_escape(path: &str) -> String {
    if path.contains(|c: char| c.is_whitespace() || "\"'\\$`!#&|;(){}[]<>?*~".contains(c)) {
        format!("'{}'", path.replace('\'', "'\\''"))
    } else {
        path.to_string()
    }
}

fn render_with_tabs(drawer: &mut GpuDrawer, tabs: &TabManager, preedit: &str, sel: &Selection, ink_state: &InkImeState, hover_url_range: Option<(u32, u16, u16)>, is_break: bool, break_text: Option<&[String]>, show_scrollbar: bool, copy_flash: Option<(u16, u16, Instant)>, tab_dragging: Option<usize>, transparent_tab_bar: bool, title_bar_height: f32, header_opacity: f32) {
    let tab = match tabs.active_tab() {
        Some(t) => t,
        None => return,
    };

    let state = tab.terminal.lock().unwrap();
    let scrolled = state.grid.scroll_offset() > 0;
    let cursor_pos = state.grid.cursor_pos();
    let cursor = if scrolled || !state.grid.cursor_visible() {
        None
    } else {
        Some(cursor_pos)
    };
    let preedit_str = if preedit.is_empty() || scrolled {
        None
    } else {
        Some(preedit)
    };

    let scrollback_len = state.grid.scrollback_len();
    let rows = state.grid.cells().len();
    let scroll_offset = state.grid.scroll_offset();
    let scrollbar = if show_scrollbar && scrollback_len > 0 {
        let total = (scrollback_len + rows) as f32;
        let thumb_height = rows as f32 / total;
        let thumb_top = (scrollback_len - scroll_offset) as f32 / total;
        Some((thumb_top, thumb_height))
    } else {
        None
    };
    let visible = state.grid.visible_cells();
    let view_base = (state
        .grid
        .scrollback_len()
        .saturating_sub(state.grid.scroll_offset())) as u32;
    let visible_rows = visible.len() as u16;
    let sel_range = sel.screen_normalized(view_base, visible_rows);

    let show_tab_bar = tabs.show_tab_bar();
    let preedit_pos_override = if preedit_str.is_some() {
        ink_state.preedit_pos(&visible)
    } else {
        None
    };
    let mut commands = growterm_render_cmd::generate_with_offset(
        &visible,
        cursor,
        preedit_str,
        sel_range,
        0,
        state.palette,
        preedit_pos_override,
        if scrolled { None } else { Some(cursor_pos) },
    );

    // Post-process: add UNDERLINE flag for hover URL range
    if let Some((abs_row, start_col, end_col)) = hover_url_range {
        if abs_row >= view_base && abs_row < view_base + visible_rows as u32 {
            let screen_row = (abs_row - view_base) as u16;
            for cmd in commands.iter_mut() {
                if cmd.row == screen_row && cmd.col >= start_col && cmd.col < end_col {
                    cmd.flags |= growterm_types::CellFlags::UNDERLINE;
                }
            }
        }
    }

    // Copy flash: briefly invert fg/bg on cursor row
    if let Some((flash_start, flash_end, flash_time)) = copy_flash {
        if flash_time.elapsed() < Duration::from_millis(150) {
            for cmd in commands.iter_mut() {
                if cmd.row >= flash_start && cmd.row <= flash_end {
                    std::mem::swap(&mut cmd.fg, &mut cmd.bg);
                }
            }
        }
    }

    drop(state);

    let tab_bar = if show_tab_bar {
        let info = tabs.tab_bar_info();
        Some(growterm_gpu_draw::TabBarInfo {
            titles: info.titles,
            active_index: info.active_index,
            dragging_index: tab_dragging,
        })
    } else {
        None
    };

    let has_scrollback = scrollback_len > 0;
    drawer.draw(&commands, scrollbar, tab_bar.as_ref(), is_break, break_text, transparent_tab_bar, has_scrollback, title_bar_height, header_opacity);
}

#[cfg(test)]
mod tests {
    use super::*;
    use growterm_types::TerminalCommand;

    #[test]
    fn shell_escape_plain_path() {
        assert_eq!(shell_escape("/Users/me/file.txt"), "/Users/me/file.txt");
    }

    #[test]
    fn shell_escape_path_with_spaces() {
        assert_eq!(
            shell_escape("/Users/me/my file.txt"),
            "'/Users/me/my file.txt'"
        );
    }

    #[test]
    fn shell_escape_path_with_special_chars() {
        assert_eq!(shell_escape("/tmp/a&b.txt"), "'/tmp/a&b.txt'");
    }

    #[test]
    fn shell_escape_path_with_single_quote() {
        assert_eq!(shell_escape("/tmp/it's.txt"), "'/tmp/it'\\''s.txt'");
    }

    fn make_grid_with_lines(cols: u16, rows: u16, lines: &[&str]) -> growterm_grid::Grid {
        use growterm_types::TerminalCommand;
        let mut grid = growterm_grid::Grid::new(cols, rows);
        for (i, line) in lines.iter().enumerate() {
            for ch in line.chars() {
                grid.apply(&TerminalCommand::Print(ch));
            }
            if i + 1 < lines.len() {
                grid.apply(&TerminalCommand::Newline);
            }
        }
        grid
    }

    #[test]
    fn extract_grid_text_from_start() {
        let grid = make_grid_with_lines(80, 24, &["$ ls", "file.txt", "$ echo hello", "hello"]);
        let text = extract_grid_text(&grid, 0);
        assert!(text.contains("$ ls"));
        assert!(text.contains("file.txt"));
        assert!(text.contains("hello"));
    }

    #[test]
    fn extract_grid_text_from_offset() {
        let grid = make_grid_with_lines(80, 24, &["old line", "$ ls", "result"]);
        // start_abs=1 should skip "old line"
        let text = extract_grid_text(&grid, 1);
        assert!(!text.contains("old line"));
        assert!(text.contains("$ ls"));
        assert!(text.contains("result"));
    }

    #[test]
    fn extract_grid_text_empty_when_no_new_output() {
        let grid = make_grid_with_lines(80, 24, &["hello"]);
        let total = grid.scrollback_len() + grid.cells().len();
        // start_abs == current_total → no new output
        let text = extract_grid_text(&grid, total);
        assert!(text.trim().is_empty());
    }

    #[test]
    fn extract_grid_text_with_scrollback() {
        // Create a small grid so lines go into scrollback
        let grid = make_grid_with_lines(80, 3, &[
            "line1", "line2", "line3", "line4", "line5",
        ]);
        // Some lines should be in scrollback now
        assert!(grid.scrollback_len() > 0, "expected scrollback");
        // Extract from start should get all content
        let text = extract_grid_text(&grid, 0);
        assert!(text.contains("line1"));
        assert!(text.contains("line5"));
    }

    /// Simulate the full pomodoro coaching flow:
    /// 1. Record scrollback snapshot at work start
    /// 2. Add terminal output during work
    /// 3. Extract text at break start
    #[test]
    fn simulate_pomodoro_coaching_flow() {
        use crate::pomodoro::Pomodoro;

        let mut pomodoro = Pomodoro::new(25 * 60, 3 * 60);
        pomodoro.toggle(); // enable

        // Create a grid with some pre-existing content (before pomodoro starts)
        let mut grid = make_grid_with_lines(80, 24, &["$ old-command", "old output"]);
        let (cursor_row, _) = grid.cursor_pos();
        let initial_abs = grid.scrollback_len() + cursor_row as usize + 1;

        // Simulate on_input: Idle -> Working, saves snapshot
        let tab_id: u64 = 0;
        pomodoro.on_input(&[(tab_id, initial_abs)]);
        assert_eq!(pomodoro.scrollback_snapshot().get(&tab_id), Some(&initial_abs));

        // Simulate terminal output during work phase
        for line in &["$ cargo build", "   Compiling foo", "   Finished dev", "$ cargo test", "test result: ok"] {
            grid.apply(&TerminalCommand::Newline);
            for ch in line.chars() {
                grid.apply(&TerminalCommand::Print(ch));
            }
        }

        // Extract text - should only contain work-phase output
        let text = extract_grid_text(&grid, initial_abs);
        assert!(!text.trim().is_empty(), "text should not be empty: '{text}'");
        assert!(text.contains("cargo build"), "should contain 'cargo build': {text}");
        assert!(text.contains("cargo test"), "should contain 'cargo test': {text}");
        assert!(!text.contains("old-command"), "should NOT contain pre-work content: {text}");
    }

    /// Simulate what happens when snapshot total equals current total (no new output)
    #[test]
    fn simulate_pomodoro_no_new_output() {
        let grid = make_grid_with_lines(80, 24, &["$ hello"]);
        let total = grid.scrollback_len() + grid.cells().len();
        // No new output after snapshot
        let text = extract_grid_text(&grid, total);
        assert!(text.trim().is_empty());
    }

    /// The original bug: only a few lines of output within screen rows.
    /// Old code used scrollback_len + cells.len() (always = screen rows),
    /// so current_total == start_abs → empty text.
    /// New code uses scrollback_len + cursor_row + 1, so new output is captured.
    #[test]
    fn coaching_captures_output_within_screen() {
        use crate::pomodoro::Pomodoro;

        let mut pomodoro = Pomodoro::new(25 * 60, 3 * 60);
        pomodoro.toggle();

        // Only 1 line before work starts, screen has 24 rows
        let mut grid = make_grid_with_lines(80, 24, &["$ prompt"]);
        let (cursor_row, _) = grid.cursor_pos();
        let snapshot_abs = grid.scrollback_len() + cursor_row as usize + 1;
        pomodoro.on_input(&[(0, snapshot_abs)]);

        // Just 1 line of new output — well within screen, no scrollback
        grid.apply(&TerminalCommand::Newline);
        for ch in "$ echo hi".chars() {
            grid.apply(&TerminalCommand::Print(ch));
        }

        let text = extract_grid_text(&grid, snapshot_abs);
        assert!(text.contains("echo hi"), "single line within screen must be captured: '{text}'");
    }

    /// Output that fills and overflows the screen into scrollback
    #[test]
    fn coaching_captures_output_with_scrollback_overflow() {
        use crate::pomodoro::Pomodoro;

        let mut pomodoro = Pomodoro::new(25 * 60, 3 * 60);
        pomodoro.toggle();

        let mut grid = growterm_grid::Grid::new(80, 5); // tiny 5-row screen
        let snapshot_abs = grid.scrollback_len() + grid.cursor_pos().0 as usize + 1;
        pomodoro.on_input(&[(0, snapshot_abs)]);

        // Add 10 lines — overflows 5-row screen, pushes into scrollback
        for i in 0..10 {
            grid.apply(&TerminalCommand::Newline);
            let line = format!("output line {i}");
            for ch in line.chars() {
                grid.apply(&TerminalCommand::Print(ch));
            }
        }

        let text = extract_grid_text(&grid, snapshot_abs);
        assert!(text.contains("output line 0"), "first line: '{text}'");
        assert!(text.contains("output line 9"), "last line: '{text}'");
    }

    #[test]
    fn extract_grid_text_skips_old_scrollback() {
        let grid = make_grid_with_lines(80, 3, &[
            "old1", "old2", "old3", "new1", "new2",
        ]);
        // Start from after the first scrollback line
        let text = extract_grid_text(&grid, 1);
        assert!(!text.contains("old1"), "should not contain old1, got: {text}");
        assert!(text.contains("new2"));
    }
}
