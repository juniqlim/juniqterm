use std::io::Write;
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use growterm_gpu_draw::{GpuDrawer, TabBarInfo};
use growterm_macos::{AppEvent, MacWindow, Modifiers};

use crate::copy_mode::CopyMode;
use crate::ink_workaround::InkImeState;
use crate::pomodoro::Pomodoro;
use crate::selection::{self, Selection};
use crate::tab::{Tab, TabManager};
use crate::url;
use crate::zoom;

pub fn run(window: Arc<MacWindow>, rx: mpsc::Receiver<AppEvent>, mut drawer: GpuDrawer) {
    let (cell_w, cell_h) = drawer.cell_size();
    let mut font_size = crate::FONT_SIZE;
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
    let mut response_timer_enabled = load_response_timer_enabled();
    if response_timer_enabled {
        if let Some(tab) = tabs.active_tab_mut() {
            tab.response_timer.set_enabled(true);
        }
        window.set_response_timer_checked(true);
    }
    let mut copy_mode = CopyMode::new();
    let mut pomodoro = Pomodoro::new();
    if load_pomodoro_enabled() {
        pomodoro.toggle();
        window.set_pomodoro_checked(true);
    }
    // hover_url_range: (abs_row, start_col, end_col) for Cmd+hover URL underline
    let mut hover_url_range: Option<(u32, u16, u16)> = None;
    let mut scrollbar_dragging = false;
    let mut scrollbar_visible_until: Option<Instant> = None;
    const SCROLLBAR_HIT_WIDTH: f32 = 20.0;
    const SCROLLBAR_SHOW_DURATION: Duration = Duration::from_millis(1500);
    // copy flash: screen row to highlight briefly after Cmd+A
    let mut copy_flash: Option<(u16, Instant)> = None;
    const COPY_FLASH_DURATION: Duration = Duration::from_millis(150);

    loop {
        let event = if let Some(evt) = deferred.take() {
            evt
        } else {
            match rx.recv() {
                Ok(evt) => evt,
                Err(_) => break,
            }
        };
        match event {
            AppEvent::TextCommit(text) => {
                preedit.clear();
                ink_state.on_text_commit(&text);
                if pomodoro.is_input_blocked() {
                    continue;
                }
                pomodoro.on_input();
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
                        render_with_tabs(&mut drawer, &tabs, &preedit, &sel, &ink_state, hover_url_range, pomodoro.is_input_blocked(), scrollbar_dragging || scrollbar_visible_until.map_or(false, |t| t > Instant::now()), copy_flash);
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
                        render_with_tabs(&mut drawer, &tabs, &preedit, &sel, &ink_state, hover_url_range, pomodoro.is_input_blocked(), scrollbar_dragging || scrollbar_visible_until.map_or(false, |t| t > Instant::now()), copy_flash);
                        continue;
                    }

                    // Cmd+Shift+[ / Cmd+Shift+]: prev/next tab
                    if modifiers.contains(Modifiers::SHIFT) {
                        if keycode == kc::ANSI_LEFT_BRACKET {
                            tabs.prev_tab();
                            if copy_mode.active { copy_mode.exit(&mut sel); window.set_copy_mode(false); }
                            sel.clear();
                            preedit.clear();
                            window.discard_marked_text();
                            render_with_tabs(&mut drawer, &tabs, &preedit, &sel, &ink_state, hover_url_range, pomodoro.is_input_blocked(), scrollbar_dragging || scrollbar_visible_until.map_or(false, |t| t > Instant::now()), copy_flash);
                            continue;
                        }
                        if keycode == kc::ANSI_RIGHT_BRACKET {
                            tabs.next_tab();
                            if copy_mode.active { copy_mode.exit(&mut sel); window.set_copy_mode(false); }
                            sel.clear();
                            preedit.clear();
                            window.discard_marked_text();
                            render_with_tabs(&mut drawer, &tabs, &preedit, &sel, &ink_state, hover_url_range, pomodoro.is_input_blocked(), scrollbar_dragging || scrollbar_visible_until.map_or(false, |t| t > Instant::now()), copy_flash);
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
                            if copy_mode.active { copy_mode.exit(&mut sel); window.set_copy_mode(false); }
                            sel.clear();
                            preedit.clear();
                            window.discard_marked_text();
                            render_with_tabs(&mut drawer, &tabs, &preedit, &sel, &ink_state, hover_url_range, pomodoro.is_input_blocked(), scrollbar_dragging || scrollbar_visible_until.map_or(false, |t| t > Instant::now()), copy_flash);
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
                        render_with_tabs(&mut drawer, &tabs, &preedit, &sel, &ink_state, hover_url_range, pomodoro.is_input_blocked(), true, copy_flash);
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
                        render_with_tabs(&mut drawer, &tabs, &preedit, &sel, &ink_state, hover_url_range, pomodoro.is_input_blocked(), true, copy_flash);
                        continue;
                    }

                    // Cmd+A: copy input line to clipboard
                    if keycode == kc::ANSI_A {
                        if let Some(tab) = tabs.active_tab() {
                            let state = tab.terminal.lock().unwrap();
                            let (text, flash_row) = selection::input_line_text(&state.grid);
                            drop(state);
                            if !text.is_empty() {
                                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                    let _ = clipboard.set_text(text);
                                }
                            }
                            copy_flash = Some((flash_row, Instant::now()));
                            render_with_tabs(&mut drawer, &tabs, &preedit, &sel, &ink_state, hover_url_range, pomodoro.is_input_blocked(), scrollbar_dragging || scrollbar_visible_until.map_or(false, |t| t > Instant::now()), copy_flash);
                            let w = window.clone();
                            std::thread::spawn(move || {
                                std::thread::sleep(COPY_FLASH_DURATION);
                                w.request_redraw();
                            });
                        }
                        continue;
                    }

                    // Cmd+Shift+C: 복사모드 진입
                    if keycode == kc::ANSI_C && modifiers.contains(Modifiers::SHIFT) {
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
                        render_with_tabs(&mut drawer, &tabs, &preedit, &sel, &ink_state, hover_url_range, pomodoro.is_input_blocked(), scrollbar_dragging || scrollbar_visible_until.map_or(false, |t| t > Instant::now()), copy_flash);
                        continue;
                    }

                    // Cmd+C copy
                    if keycode == kc::ANSI_C {
                        if !sel.is_empty() {
                            if let Some(tab) = tabs.active_tab() {
                                let state = tab.terminal.lock().unwrap();
                                let text = selection::extract_text_absolute(&state.grid, &sel);
                                drop(state);
                                if !text.is_empty() {
                                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                        let _ = clipboard.set_text(text);
                                    }
                                }
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
                        // Resize all tabs
                        for tab in tabs.tabs_mut() {
                            let mut state = tab.terminal.lock().unwrap();
                            state.grid.resize(cols, term_rows);
                            drop(state);
                            let _ = tab.pty_writer.resize(term_rows, cols);
                        }
                        render_with_tabs(&mut drawer, &tabs, &preedit, &sel, &ink_state, hover_url_range, pomodoro.is_input_blocked(), scrollbar_dragging || scrollbar_visible_until.map_or(false, |t| t > Instant::now()), copy_flash);
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

                    match keycode {
                        kc::ANSI_J => {
                            copy_mode.move_down(cols, max_row, &mut sel);
                        }
                        kc::ANSI_K => {
                            copy_mode.move_up(cols, &mut sel);
                        }
                        kc::ANSI_V => {
                            copy_mode.toggle_visual(cols, &mut sel);
                        }
                        kc::ANSI_H => {
                            copy_mode.move_right(cols, max_row, &mut sel);
                        }
                        kc::ANSI_L => {
                            copy_mode.move_left(cols, max_row, &mut sel);
                        }
                        kc::ANSI_Y => {
                            // y: 선택 텍스트 클립보드에 복사 후 모드 종료
                            if !sel.is_empty() {
                                if let Some(tab) = tabs.active_tab() {
                                    let state = tab.terminal.lock().unwrap();
                                    let text = selection::extract_text_absolute(&state.grid, &sel);
                                    drop(state);
                                    if !text.is_empty() {
                                        if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                            let _ = clipboard.set_text(text);
                                        }
                                    }
                                }
                            }
                            copy_mode.exit(&mut sel);
                            window.set_copy_mode(false);
                        }
                        kc::ESCAPE | kc::ANSI_Q => {
                            copy_mode.exit(&mut sel);
                            window.set_copy_mode(false);
                        }
                        _ => {}
                    }

                    // 커서 행이 화면에 보이도록 스크롤 조정
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

                    render_with_tabs(&mut drawer, &tabs, &preedit, &sel, &ink_state, hover_url_range, pomodoro.is_input_blocked(), scrollbar_dragging || scrollbar_visible_until.map_or(false, |t| t > Instant::now()), copy_flash);
                    continue;
                }

                if pomodoro.is_input_blocked() {
                    continue;
                }
                if let Some(key_event) =
                    growterm_macos::convert_key(keycode, characters.as_deref(), modifiers)
                {
                    let bytes = growterm_input::encode(key_event);
                    pomodoro.on_input();
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

                // Tab bar click: switch to clicked tab
                if tabs.show_tab_bar() && (y as f32) < ch {
                    let screen_w = window.inner_size().0 as f32;
                    if let Some(index) = tabs.tab_index_at_x(x as f32, screen_w) {
                        tabs.switch_to(index);
                        preedit.clear();
                        window.discard_marked_text();
                        window.request_redraw();
                    }
                    continue;
                }

                // Mouse tracking: send SGR report to PTY
                {
                    let y_offset = tabs.mouse_y_offset(drawer.tab_bar_height());
                    if let Some(tab) = tabs.active_tab_mut() {
                        let mode = tab.mouse_mode.load(Ordering::Relaxed);
                        if mode > 0 {
                            let (row, col) = selection::pixel_to_cell(
                                x as f32, y as f32 - y_offset, cw, ch,
                            );
                            let seq = format!("\x1b[<0;{};{}M", col as u32 + 1, row as u32 + 1);
                            let _ = tab.pty_writer.write_all(seq.as_bytes());
                            let _ = tab.pty_writer.flush();
                            continue;
                        }
                    }
                }

                // Scrollbar area click: start dragging
                let screen_w = window.inner_size().0 as f32;
                let screen_h = window.inner_size().1 as f32;
                if (x as f32) >= screen_w - SCROLLBAR_HIT_WIDTH {
                    if let Some(tab) = tabs.active_tab() {
                        let mut state = tab.terminal.lock().unwrap();
                        let scrollback_len = state.grid.scrollback_len();
                        if scrollback_len > 0 {
                            scrollbar_dragging = true;
                            scrollbar_visible_until = Some(Instant::now() + SCROLLBAR_SHOW_DURATION);
                            let tab_bar_offset = tabs.mouse_y_offset(drawer.tab_bar_height());
                            let content_h = screen_h - tab_bar_offset;
                            let ratio = ((y as f32) - tab_bar_offset).clamp(0.0, content_h) / content_h;
                            let rows = state.grid.cells().len();
                            let total = scrollback_len + rows;
                            let target_top_row = (ratio * total as f32) as usize;
                            let offset = scrollback_len.saturating_sub(target_top_row).min(scrollback_len);
                            state.grid.set_scroll_offset(offset);
                            drop(state);
                            render_with_tabs(&mut drawer, &tabs, &preedit, &sel, &ink_state, hover_url_range, pomodoro.is_input_blocked(), true, copy_flash);
                            continue;
                        }
                    }
                }

                let (screen_row, col) =
                    selection::pixel_to_cell(x as f32, y as f32 - tabs.mouse_y_offset(drawer.tab_bar_height()), cw, ch);
                let abs_row = if let Some(tab) = tabs.active_tab() {
                    let state = tab.terminal.lock().unwrap();
                    let base = state
                        .grid
                        .scrollback_len()
                        .saturating_sub(state.grid.scroll_offset());
                    screen_row as u32 + base as u32
                } else {
                    screen_row as u32
                };

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
                // Mouse tracking: send SGR drag report to PTY
                {
                    let y_offset = tabs.mouse_y_offset(drawer.tab_bar_height());
                    if let Some(tab) = tabs.active_tab_mut() {
                        let mode = tab.mouse_mode.load(Ordering::Relaxed);
                        if mode >= 2 {
                            let (cw, ch) = drawer.cell_size();
                            let (row, col) = selection::pixel_to_cell(
                                x as f32, y as f32 - y_offset, cw, ch,
                            );
                            // btn 32 = motion flag + button 0
                            let seq = format!("\x1b[<32;{};{}M", col as u32 + 1, row as u32 + 1);
                            let _ = tab.pty_writer.write_all(seq.as_bytes());
                            let _ = tab.pty_writer.flush();
                            continue;
                        }
                    }
                }
                if scrollbar_dragging {
                    let screen_h = window.inner_size().1 as f32;
                    let tab_bar_offset = tabs.mouse_y_offset(drawer.tab_bar_height());
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
                        drop(state);
                        scrollbar_visible_until = Some(Instant::now() + SCROLLBAR_SHOW_DURATION);
                        render_with_tabs(&mut drawer, &tabs, &preedit, &sel, &ink_state, hover_url_range, pomodoro.is_input_blocked(), true, copy_flash);
                    }
                } else if sel.active {
                    let (cw, ch) = drawer.cell_size();
                    let (screen_row, col) = selection::pixel_to_cell(
                        x as f32,
                        y as f32 - tabs.mouse_y_offset(drawer.tab_bar_height()),
                        cw,
                        ch,
                    );
                    let abs_row = if let Some(tab) = tabs.active_tab() {
                        let state = tab.terminal.lock().unwrap();
                        let base = state
                            .grid
                            .scrollback_len()
                            .saturating_sub(state.grid.scroll_offset());
                        screen_row as u32 + base as u32
                    } else {
                        screen_row as u32
                    };
                    sel.update(abs_row, col);
                    window.request_redraw();
                }
            }
            AppEvent::MouseUp(x, y) => {
                if scrollbar_dragging {
                    scrollbar_dragging = false;
                    continue;
                }
                let (cw, ch) = drawer.cell_size();
                if tabs.show_tab_bar() && (y as f32) < ch {
                    continue;
                }

                // Mouse tracking: send SGR release report to PTY
                {
                    let y_offset = tabs.mouse_y_offset(drawer.tab_bar_height());
                    if let Some(tab) = tabs.active_tab_mut() {
                        let mode = tab.mouse_mode.load(Ordering::Relaxed);
                        if mode > 0 {
                            let (row, col) = selection::pixel_to_cell(
                                x as f32, y as f32 - y_offset, cw, ch,
                            );
                            let seq = format!("\x1b[<0;{};{}m", col as u32 + 1, row as u32 + 1);
                            let _ = tab.pty_writer.write_all(seq.as_bytes());
                            let _ = tab.pty_writer.flush();
                            continue;
                        }
                    }
                }

                let (screen_row, col) =
                    selection::pixel_to_cell(x as f32, y as f32 - tabs.mouse_y_offset(drawer.tab_bar_height()), cw, ch);
                let abs_row = if let Some(tab) = tabs.active_tab() {
                    let state = tab.terminal.lock().unwrap();
                    let base = state
                        .grid
                        .scrollback_len()
                        .saturating_sub(state.grid.scroll_offset());
                    screen_row as u32 + base as u32
                } else {
                    screen_row as u32
                };
                sel.update(abs_row, col);
                sel.finish();
                window.request_redraw();
            }
            AppEvent::MouseMoved(x, y, modifiers) => {
                let new_range = if modifiers.contains(Modifiers::SUPER) {
                    let (cw, ch) = drawer.cell_size();
                    let (screen_row, col) = selection::pixel_to_cell(
                        x as f32,
                        y as f32 - tabs.mouse_y_offset(drawer.tab_bar_height()),
                        cw,
                        ch,
                    );
                    if let Some(tab) = tabs.active_tab() {
                        let state = tab.terminal.lock().unwrap();
                        let base = state
                            .grid
                            .scrollback_len()
                            .saturating_sub(state.grid.scroll_offset());
                        let abs_row = screen_row as u32 + base as u32;
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
                    render_with_tabs(&mut drawer, &tabs, &preedit, &sel, &ink_state, hover_url_range, pomodoro.is_input_blocked(), true, copy_flash);
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
                for tab in tabs.tabs_mut() {
                    let mut state = tab.terminal.lock().unwrap();
                    state.grid.resize(cols, term_rows);
                    drop(state);
                    let _ = tab.pty_writer.resize(term_rows, cols);
                }
                render_with_tabs(&mut drawer, &tabs, &preedit, &sel, &ink_state, hover_url_range, pomodoro.is_input_blocked(), scrollbar_dragging || scrollbar_visible_until.map_or(false, |t| t > Instant::now()), copy_flash);
            }
            AppEvent::RedrawRequested => {
                // Expire copy flash
                if let Some((_, t)) = copy_flash {
                    if t.elapsed() >= COPY_FLASH_DURATION {
                        copy_flash = None;
                    }
                }
                pomodoro.tick();
                // Feed PTY output timestamp to each tab's response timer
                for tab in tabs.tabs_mut() {
                    let ts = tab.last_pty_output_at.lock().unwrap().take();
                    if let Some(ts) = ts {
                        tab.response_timer.on_pty_output(ts);
                    }
                    tab.response_timer.tick();
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
                render_with_tabs(&mut drawer, &tabs, &preedit, &sel, &ink_state, hover_url_range, pomodoro.is_input_blocked(), scrollbar_dragging || scrollbar_visible_until.map_or(false, |t| t > Instant::now()), copy_flash);
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
                save_pomodoro_enabled(enabled);
                window.set_pomodoro_checked(enabled);
                let title = build_title(&pomodoro, &tabs);
                window.set_title(&title);
            }
            AppEvent::ToggleResponseTimer => {
                response_timer_enabled = !response_timer_enabled;
                for tab in tabs.tabs_mut() {
                    tab.response_timer.set_enabled(response_timer_enabled);
                }
                save_response_timer_enabled(response_timer_enabled);
                window.set_response_timer_checked(response_timer_enabled);
                let title = build_title(&pomodoro, &tabs);
                window.set_title(&title);
            }
            AppEvent::CloseRequested => {
                std::process::exit(0);
            }
        }
    }
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

fn pomodoro_config_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    std::path::PathBuf::from(home)
        .join(".config")
        .join("growterm")
        .join("pomodoro_enabled")
}

fn load_pomodoro_enabled() -> bool {
    std::fs::read_to_string(pomodoro_config_path())
        .map(|s| s.trim() == "1")
        .unwrap_or(false)
}

fn save_pomodoro_enabled(enabled: bool) {
    let path = pomodoro_config_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, if enabled { "1" } else { "0" });
}

fn response_timer_config_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    std::path::PathBuf::from(home)
        .join(".config")
        .join("growterm")
        .join("response_timer_enabled")
}

fn load_response_timer_enabled() -> bool {
    std::fs::read_to_string(response_timer_config_path())
        .map(|s| s.trim() == "1")
        .unwrap_or(false)
}

fn save_response_timer_enabled(enabled: bool) {
    let path = response_timer_config_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, if enabled { "1" } else { "0" });
}

fn shell_escape(path: &str) -> String {
    if path.contains(|c: char| c.is_whitespace() || "\"'\\$`!#&|;(){}[]<>?*~".contains(c)) {
        format!("'{}'", path.replace('\'', "'\\''"))
    } else {
        path.to_string()
    }
}

fn render_with_tabs(drawer: &mut GpuDrawer, tabs: &TabManager, preedit: &str, sel: &Selection, ink_state: &InkImeState, hover_url_range: Option<(u32, u16, u16)>, is_break: bool, show_scrollbar: bool, copy_flash: Option<(u16, Instant)>) {
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
    if let Some((flash_row, flash_time)) = copy_flash {
        if flash_time.elapsed() < Duration::from_millis(150) {
            for cmd in commands.iter_mut() {
                if cmd.row == flash_row {
                    std::mem::swap(&mut cmd.fg, &mut cmd.bg);
                }
            }
        }
    }

    drop(state);

    let tab_bar = if show_tab_bar {
        Some(TabBarInfo {
            titles: tabs.tab_bar_info().titles,
            active_index: tabs.tab_bar_info().active_index,
        })
    } else {
        None
    };

    drawer.draw(&commands, scrollbar, tab_bar.as_ref(), is_break);
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
