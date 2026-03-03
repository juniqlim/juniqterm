use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use growterm_grid::Grid;
use growterm_macos::MacWindow;
use growterm_pty::PtyWriter;
use growterm_render_cmd::TerminalPalette;
use growterm_types::Rgb;
use growterm_vt_parser::VtParser;

use crate::response_timer::ResponseTimer;

pub struct Tab {
    pub id: u64,
    pub terminal: Arc<Mutex<TerminalState>>,
    pub pty_writer: PtyWriter,
    pub dirty: Arc<AtomicBool>,
    pub sync_output: Arc<AtomicBool>,
    pub last_pty_output_at: Arc<Mutex<Option<Instant>>>,
    pub response_timer: ResponseTimer,
    pub bracketed_paste: Arc<AtomicBool>,
    pub mouse_mode: Arc<AtomicU8>,
}

pub struct TerminalState {
    pub grid: Grid,
    pub vt_parser: VtParser,
    pub palette: TerminalPalette,
}

pub struct TabManager {
    tabs: Vec<Tab>,
    active: usize,
    next_id: u64,
}

/// Info passed to the renderer for drawing the tab bar.
pub struct TabBarInfo {
    pub titles: Vec<String>,
    pub active_index: usize,
}

/// Compute content Y offset — shared between renderer and mouse coordinate translation.
/// Must match the renderer's y_off logic exactly.
/// Returns true if the given y pixel coordinate falls within the tab bar region.
pub fn hit_test_tab_bar(y: f32, tab_bar_h: f32, content_y_off: f32) -> bool {
    let top = content_y_off - tab_bar_h;
    y >= top && y < content_y_off
}

pub fn content_y_offset(show_tab_bar: bool, tab_bar_h: f32, title_bar_h: f32, screen_full: bool) -> f32 {
    let transparent = title_bar_h > 0.0;
    if transparent && screen_full {
        0.0
    } else if !show_tab_bar {
        if transparent { title_bar_h } else { 0.0 }
    } else if transparent {
        title_bar_h + tab_bar_h
    } else {
        tab_bar_h
    }
}

impl TabManager {
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active: 0,
            next_id: 0,
        }
    }

    pub fn add_tab(&mut self, mut tab: Tab) {
        tab.id = self.next_id;
        self.next_id += 1;
        let insert_at = if self.tabs.is_empty() {
            0
        } else {
            self.active + 1
        };
        self.tabs.insert(insert_at, tab);
        self.active = insert_at;
    }

    pub fn close_tab(&mut self, index: usize) -> Option<Tab> {
        if index >= self.tabs.len() {
            return None;
        }
        let tab = self.tabs.remove(index);
        if self.tabs.is_empty() {
            // caller should handle exit
        } else if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        } else if self.active > index {
            self.active -= 1;
        }
        Some(tab)
    }

    pub fn close_active(&mut self) -> Option<Tab> {
        let idx = self.active;
        self.close_tab(idx)
    }

    pub fn switch_to(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active = index;
        }
    }

    pub fn next_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active = (self.active + 1) % self.tabs.len();
        }
    }

    pub fn prev_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active = if self.active == 0 {
                self.tabs.len() - 1
            } else {
                self.active - 1
            };
        }
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.active)
    }

    #[allow(dead_code)]
    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    #[allow(dead_code)]
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub fn tabs_mut(&mut self) -> &mut [Tab] {
        &mut self.tabs
    }

    pub fn show_tab_bar(&self) -> bool {
        self.tabs.len() > 1
    }

    /// Terminal rows adjusted for tab bar presence.
    pub fn term_rows(&self, screen_h: u32, cell_h: f32, tab_bar_h: f32) -> u16 {
        if self.show_tab_bar() {
            ((screen_h as f32 - tab_bar_h) / cell_h).floor().max(1.0) as u16
        } else {
            (screen_h as f32 / cell_h).floor().max(1.0) as u16
        }
    }

    /// Y pixel offset for mouse events — mirrors renderer y_off logic.
    pub fn mouse_y_offset(&self, tab_bar_h: f32, title_bar_h: f32, screen_full: bool) -> f32 {
        content_y_offset(self.show_tab_bar(), tab_bar_h, title_bar_h, screen_full)
    }

    pub fn move_tab(&mut self, from: usize, to: usize) {
        if from >= self.tabs.len() || to >= self.tabs.len() || from == to {
            return;
        }
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        if self.active == from {
            self.active = to;
        } else if from < to {
            // moved right: indices [from+1..=to] shifted left by 1
            if self.active > from && self.active <= to {
                self.active -= 1;
            }
        } else {
            // moved left: indices [to..from-1] shifted right by 1
            if self.active >= to && self.active < from {
                self.active += 1;
            }
        }
    }

    /// Returns the tab index at pixel x, given the screen width.
    /// Returns `None` if the tab bar is not shown or x is out of range.
    pub fn tab_index_at_x(&self, x: f32, screen_w: f32) -> Option<usize> {
        if !self.show_tab_bar() || self.tabs.is_empty() || screen_w <= 0.0 {
            return None;
        }
        let tab_w = screen_w / self.tabs.len() as f32;
        let index = (x / tab_w) as usize;
        if index < self.tabs.len() {
            Some(index)
        } else {
            None
        }
    }

    pub fn tab_bar_info(&self) -> TabBarInfo {
        TabBarInfo {
            titles: self
                .tabs
                .iter()
                .enumerate()
                .map(|(idx, tab)| {
                    let num = idx + 1;
                    let label = if num <= 9 {
                        format!("⌘{}", num)
                    } else {
                        format!("{}", num)
                    };
                    if let Some(timer_text) = tab.response_timer.display_text() {
                        format!("{} {}", label, timer_text)
                    } else {
                        label
                    }
                })
                .collect(),
            active_index: self.active,
        }
    }
}

impl Tab {
    pub fn spawn(rows: u16, cols: u16, window: Arc<MacWindow>) -> Result<Self, std::io::Error> {
        Self::spawn_with_cwd(rows, cols, window, None)
    }

    pub fn spawn_with_cwd(
        rows: u16,
        cols: u16,
        window: Arc<MacWindow>,
        cwd: Option<&std::path::Path>,
    ) -> Result<Self, std::io::Error> {
        let grid = Grid::new(cols, rows);
        let vt_parser = VtParser::new();
        let terminal = Arc::new(Mutex::new(TerminalState {
            grid,
            vt_parser,
            palette: TerminalPalette::default(),
        }));
        let dirty = Arc::new(AtomicBool::new(false));
        let sync_output = Arc::new(AtomicBool::new(false));
        let last_pty_output_at = Arc::new(Mutex::new(None));
        let bracketed_paste = Arc::new(AtomicBool::new(false));
        let mouse_mode = Arc::new(AtomicU8::new(0));
        let mouse_sgr = Arc::new(AtomicBool::new(false));
        let pty_writer = match growterm_pty::spawn_with_cwd(rows, cols, cwd) {
            Ok((reader, writer)) => {
                let responder = writer.responder();
                start_io_thread(
                    reader,
                    responder,
                    Arc::clone(&terminal),
                    Arc::clone(&dirty),
                    Arc::clone(&sync_output),
                    Arc::clone(&last_pty_output_at),
                    Arc::clone(&bracketed_paste),
                    Arc::clone(&mouse_mode),
                    Arc::clone(&mouse_sgr),
                    window,
                );
                writer
            }
            Err(e) => return Err(e),
        };

        Ok(Tab {
            id: 0, // assigned by TabManager::add_tab
            terminal,
            pty_writer,
            dirty,
            sync_output,
            last_pty_output_at,
            response_timer: ResponseTimer::new(),
            bracketed_paste,
            mouse_mode,
        })
    }
}

fn start_io_thread(
    mut reader: growterm_pty::PtyReader,
    responder: growterm_pty::PtyResponder,
    terminal: Arc<Mutex<TerminalState>>,
    dirty: Arc<AtomicBool>,
    sync_output: Arc<AtomicBool>,
    last_pty_output_at: Arc<Mutex<Option<Instant>>>,
    bracketed_paste: Arc<AtomicBool>,
    mouse_mode: Arc<AtomicU8>,
    mouse_sgr: Arc<AtomicBool>,
    window: Arc<MacWindow>,
) {
    std::thread::spawn(move || {
        let mut buf = [0u8; 65536];
        let mut pending_queries: Vec<u8> = Vec::new();
        let mut kitty_keyboard_flags: u16 = 0;
        let mut kitty_keyboard_stack: Vec<u16> = Vec::new();
        // sync_output is now a shared Arc<AtomicBool> passed as parameter
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    *last_pty_output_at.lock().unwrap() = Some(Instant::now());
                    pending_queries.extend_from_slice(&buf[..n]);
                    let controls = extract_terminal_controls(&mut pending_queries);

                    let mut responses = Vec::new();
                    let mut state = terminal.lock().unwrap();
                    let commands = state.vt_parser.parse(&buf[..n]);
                    for cmd in &commands {
                        state.grid.apply(cmd);
                    }
                    if state.grid.scroll_offset() == 0 {
                        state.grid.reset_scroll();
                    }
                    let cursor = state.grid.cursor_pos();
                    for control in controls {
                        match control {
                            TerminalControl::Query(query) => {
                                let response = encode_terminal_query_response(
                                    query,
                                    cursor,
                                    kitty_keyboard_flags,
                                    state.palette,
                                );
                                responses.push(response);
                            }
                            TerminalControl::KittyKeyboardPush(flags) => {
                                kitty_keyboard_stack.push(kitty_keyboard_flags);
                                kitty_keyboard_flags = flags;
                            }
                            TerminalControl::KittyKeyboardPop(count) => {
                                let mut remaining = count.max(1);
                                while remaining > 0 {
                                    if let Some(prev) = kitty_keyboard_stack.pop() {
                                        kitty_keyboard_flags = prev;
                                    } else {
                                        kitty_keyboard_flags = 0;
                                        break;
                                    }
                                    remaining -= 1;
                                }
                            }
                            TerminalControl::SetDefaultForegroundColor(color) => {
                                state.palette.default_fg = color;
                            }
                            TerminalControl::SetDefaultBackgroundColor(color) => {
                                state.palette.default_bg = color;
                            }
                            TerminalControl::SyncOutputBegin => {
                                sync_output.store(true, Ordering::Relaxed);
                            }
                            TerminalControl::SyncOutputEnd => {
                                sync_output.store(false, Ordering::Relaxed);
                            }
                            TerminalControl::BracketedPasteEnable => {
                                bracketed_paste.store(true, Ordering::Relaxed);
                            }
                            TerminalControl::BracketedPasteDisable => {
                                bracketed_paste.store(false, Ordering::Relaxed);
                            }
                            TerminalControl::MouseModeSet(mode) => {
                                mouse_mode.store(mode, Ordering::Relaxed);
                            }
                            TerminalControl::MouseSgrEnable => {
                                mouse_sgr.store(true, Ordering::Relaxed);
                            }
                            TerminalControl::MouseSgrDisable => {
                                mouse_sgr.store(false, Ordering::Relaxed);
                            }
                        }
                    }
                    drop(state);

                    for response in responses {
                        let _ = responder.write_all_flush(response.as_bytes());
                    }

                    if !sync_output.load(Ordering::Relaxed) {
                        // Only request redraw if dirty was previously false.
                        // This coalesces multiple PTY reads into a single
                        // redraw, avoiding redundant dispatch_async_f overhead.
                        let was_clean = !dirty.swap(true, Ordering::Relaxed);
                        if was_clean {
                            window.request_redraw();
                        }
                    }
                }
                Err(e) => {
                    if e.raw_os_error() == Some(libc::EIO) {
                        break;
                    }
                    break;
                }
            }
        }
        window.request_redraw();
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalQuery {
    CursorPositionReport,
    PrimaryDeviceAttributes,
    SecondaryDeviceAttributes,
    KittyKeyboardQuery,
    ForegroundColorQuery,
    BackgroundColorQuery,
    RequestStatusStringSgr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalControl {
    Query(TerminalQuery),
    KittyKeyboardPush(u16),
    KittyKeyboardPop(u16),
    SetDefaultForegroundColor(Rgb),
    SetDefaultBackgroundColor(Rgb),
    SyncOutputBegin,
    SyncOutputEnd,
    BracketedPasteEnable,
    BracketedPasteDisable,
    MouseModeSet(u8),
    MouseSgrEnable,
    MouseSgrDisable,
}

fn extract_terminal_controls(pending: &mut Vec<u8>) -> Vec<TerminalControl> {
    let mut controls = Vec::new();
    let mut i = 0usize;
    let mut keep_from = None;

    while i < pending.len() {
        if pending[i] != 0x1b {
            i += 1;
            continue;
        }

        let rest = &pending[i..];
        if rest.starts_with(b"\x1b[6n") {
            controls.push(TerminalControl::Query(TerminalQuery::CursorPositionReport));
            i += 4;
            continue;
        }
        if rest.starts_with(b"\x1b[?2026h") {
            controls.push(TerminalControl::SyncOutputBegin);
            i += 8;
            continue;
        }
        if rest.starts_with(b"\x1b[?2026l") {
            controls.push(TerminalControl::SyncOutputEnd);
            i += 8;
            continue;
        }
        if rest.starts_with(b"\x1b[?2004h") {
            controls.push(TerminalControl::BracketedPasteEnable);
            i += 8;
            continue;
        }
        if rest.starts_with(b"\x1b[?2004l") {
            controls.push(TerminalControl::BracketedPasteDisable);
            i += 8;
            continue;
        }
        // Mouse tracking modes: ?1000, ?1002, ?1003
        if rest.starts_with(b"\x1b[?1000h") {
            controls.push(TerminalControl::MouseModeSet(1));
            i += 8;
            continue;
        }
        if rest.starts_with(b"\x1b[?1000l") {
            controls.push(TerminalControl::MouseModeSet(0));
            i += 8;
            continue;
        }
        if rest.starts_with(b"\x1b[?1002h") {
            controls.push(TerminalControl::MouseModeSet(2));
            i += 8;
            continue;
        }
        if rest.starts_with(b"\x1b[?1002l") {
            controls.push(TerminalControl::MouseModeSet(0));
            i += 8;
            continue;
        }
        if rest.starts_with(b"\x1b[?1003h") {
            controls.push(TerminalControl::MouseModeSet(3));
            i += 8;
            continue;
        }
        if rest.starts_with(b"\x1b[?1003l") {
            controls.push(TerminalControl::MouseModeSet(0));
            i += 8;
            continue;
        }
        // SGR mouse encoding: ?1006
        if rest.starts_with(b"\x1b[?1006h") {
            controls.push(TerminalControl::MouseSgrEnable);
            i += 8;
            continue;
        }
        if rest.starts_with(b"\x1b[?1006l") {
            controls.push(TerminalControl::MouseSgrDisable);
            i += 8;
            continue;
        }
        if rest.starts_with(b"\x1b[?u") {
            controls.push(TerminalControl::Query(TerminalQuery::KittyKeyboardQuery));
            i += 4;
            continue;
        }
        if rest.starts_with(b"\x1b[c") {
            controls.push(TerminalControl::Query(
                TerminalQuery::PrimaryDeviceAttributes,
            ));
            i += 3;
            continue;
        }
        if rest.starts_with(b"\x1b[>c") {
            controls.push(TerminalControl::Query(
                TerminalQuery::SecondaryDeviceAttributes,
            ));
            i += 4;
            continue;
        }
        if rest.starts_with(b"\x1b[>0c") {
            controls.push(TerminalControl::Query(
                TerminalQuery::SecondaryDeviceAttributes,
            ));
            i += 5;
            continue;
        }
        if rest.starts_with(b"\x1b]10;?\x1b\\") {
            controls.push(TerminalControl::Query(TerminalQuery::ForegroundColorQuery));
            i += 8;
            continue;
        }
        if rest.starts_with(b"\x1b]10;?\x07") {
            controls.push(TerminalControl::Query(TerminalQuery::ForegroundColorQuery));
            i += 7;
            continue;
        }
        if rest.starts_with(b"\x1b]11;?\x1b\\") {
            controls.push(TerminalControl::Query(TerminalQuery::BackgroundColorQuery));
            i += 8;
            continue;
        }
        if rest.starts_with(b"\x1b]11;?\x07") {
            controls.push(TerminalControl::Query(TerminalQuery::BackgroundColorQuery));
            i += 7;
            continue;
        }
        if rest.starts_with(b"\x1bP$qm\x1b\\") {
            controls.push(TerminalControl::Query(
                TerminalQuery::RequestStatusStringSgr,
            ));
            i += 7;
            continue;
        }
        if rest.starts_with(b"\x1b]10;") || rest.starts_with(b"\x1b]11;") {
            match parse_osc_default_color_control(rest) {
                SequenceParse::Matched(control, consumed) => {
                    controls.push(control);
                    i += consumed;
                    continue;
                }
                SequenceParse::NeedMore => {
                    keep_from = Some(i);
                    break;
                }
                SequenceParse::NoMatch => {
                    if let Some(consumed) = osc_sequence_len(rest) {
                        i += consumed;
                        continue;
                    }
                    keep_from = Some(i);
                    break;
                }
            }
        }

        match parse_kitty_keyboard_control(rest) {
            SequenceParse::Matched(control, consumed) => {
                controls.push(control);
                i += consumed;
                continue;
            }
            SequenceParse::NeedMore => {
                keep_from = Some(i);
                break;
            }
            SequenceParse::NoMatch => {}
        }

        if is_known_control_prefix(rest) {
            keep_from = Some(i);
            break;
        }

        i += 1;
    }

    if let Some(start) = keep_from {
        pending.drain(..start);
    } else {
        pending.clear();
    }

    controls
}

fn is_known_control_prefix(rest: &[u8]) -> bool {
    [
        b"\x1b[?2026h".as_slice(),
        b"\x1b[?2026l".as_slice(),
        b"\x1b[?2004h".as_slice(),
        b"\x1b[?2004l".as_slice(),
        b"\x1b[?1000h".as_slice(),
        b"\x1b[?1000l".as_slice(),
        b"\x1b[?1002h".as_slice(),
        b"\x1b[?1002l".as_slice(),
        b"\x1b[?1003h".as_slice(),
        b"\x1b[?1003l".as_slice(),
        b"\x1b[?1006h".as_slice(),
        b"\x1b[?1006l".as_slice(),
        b"\x1b[6n".as_slice(),
        b"\x1b[?u".as_slice(),
        b"\x1b[c".as_slice(),
        b"\x1b[>c".as_slice(),
        b"\x1b[>0c".as_slice(),
        b"\x1b]10;?\x1b\\".as_slice(),
        b"\x1b]10;?\x07".as_slice(),
        b"\x1b]11;?\x1b\\".as_slice(),
        b"\x1b]11;?\x07".as_slice(),
        b"\x1bP$qm\x1b\\".as_slice(),
    ]
    .iter()
    .any(|pat| pat.starts_with(rest))
        || b"\x1b]10;".starts_with(rest)
        || b"\x1b]11;".starts_with(rest)
        || is_kitty_keyboard_control_prefix(rest)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SequenceParse<T> {
    Matched(T, usize),
    NeedMore,
    NoMatch,
}

fn parse_kitty_keyboard_control(rest: &[u8]) -> SequenceParse<TerminalControl> {
    if !rest.starts_with(b"\x1b[") {
        return SequenceParse::NoMatch;
    }
    if rest.len() < 3 {
        return SequenceParse::NeedMore;
    }
    let mode = rest[2];
    if mode != b'>' && mode != b'<' {
        return SequenceParse::NoMatch;
    }
    if rest.len() == 3 {
        return SequenceParse::NeedMore;
    }

    let mut idx = 3usize;
    while idx < rest.len() && rest[idx].is_ascii_digit() {
        idx += 1;
    }
    if idx == rest.len() {
        return SequenceParse::NeedMore;
    }
    if rest[idx] != b'u' {
        return SequenceParse::NoMatch;
    }

    let digits = &rest[3..idx];
    if mode == b'>' && digits.is_empty() {
        return SequenceParse::NoMatch;
    }

    let value = if digits.is_empty() {
        1
    } else {
        parse_u16_saturating(digits)
    };

    let control = if mode == b'>' {
        TerminalControl::KittyKeyboardPush(value)
    } else {
        TerminalControl::KittyKeyboardPop(value)
    };
    SequenceParse::Matched(control, idx + 1)
}

fn is_kitty_keyboard_control_prefix(rest: &[u8]) -> bool {
    if !b"\x1b[".starts_with(rest) {
        return false;
    }
    if rest.len() <= 2 {
        return true;
    }
    let mode = rest[2];
    if mode != b'>' && mode != b'<' {
        return false;
    }
    rest[3..]
        .iter()
        .all(|byte| byte.is_ascii_digit() || *byte == b'u')
}

fn parse_osc_default_color_control(rest: &[u8]) -> SequenceParse<TerminalControl> {
    let prefix = if rest.starts_with(b"\x1b]10;") {
        (5usize, true)
    } else if rest.starts_with(b"\x1b]11;") {
        (5usize, false)
    } else {
        return SequenceParse::NoMatch;
    };

    let (payload_start, is_foreground) = prefix;
    if rest.len() <= payload_start {
        return SequenceParse::NeedMore;
    }

    let Some((terminator_index, terminator_len)) = find_osc_terminator(rest) else {
        return SequenceParse::NeedMore;
    };
    let payload = &rest[payload_start..terminator_index];
    let Some(color) = parse_osc_color(payload) else {
        return SequenceParse::NoMatch;
    };

    let control = if is_foreground {
        TerminalControl::SetDefaultForegroundColor(color)
    } else {
        TerminalControl::SetDefaultBackgroundColor(color)
    };
    SequenceParse::Matched(control, terminator_index + terminator_len)
}

fn osc_sequence_len(rest: &[u8]) -> Option<usize> {
    find_osc_terminator(rest)
        .map(|(terminator_index, terminator_len)| terminator_index + terminator_len)
}

fn find_osc_terminator(rest: &[u8]) -> Option<(usize, usize)> {
    let mut idx = 0usize;
    while idx < rest.len() {
        match rest[idx] {
            0x07 => return Some((idx, 1)),
            0x1b => {
                if idx + 1 >= rest.len() {
                    return None;
                }
                if rest[idx + 1] == b'\\' {
                    return Some((idx, 2));
                }
            }
            _ => {}
        }
        idx += 1;
    }
    None
}

fn parse_osc_color(payload: &[u8]) -> Option<Rgb> {
    let text = std::str::from_utf8(payload).ok()?.trim();
    if let Some(rgb) = text.strip_prefix("rgb:") {
        let parts: Vec<&str> = rgb.split('/').collect();
        if parts.len() != 3 {
            return None;
        }
        return Some(Rgb::new(
            parse_scaled_hex(parts[0])?,
            parse_scaled_hex(parts[1])?,
            parse_scaled_hex(parts[2])?,
        ));
    }

    if let Some(hex) = text.strip_prefix('#') {
        if hex.is_empty() || hex.len() % 3 != 0 {
            return None;
        }
        let comp_len = hex.len() / 3;
        if comp_len == 0 || comp_len > 4 {
            return None;
        }
        return Some(Rgb::new(
            parse_scaled_hex(&hex[..comp_len])?,
            parse_scaled_hex(&hex[comp_len..comp_len * 2])?,
            parse_scaled_hex(&hex[comp_len * 2..])?,
        ));
    }

    None
}

fn parse_scaled_hex(hex: &str) -> Option<u8> {
    if hex.is_empty() || hex.len() > 4 {
        return None;
    }
    let value = u16::from_str_radix(hex, 16).ok()? as u32;
    let max = (1u32 << (hex.len() * 4)) - 1;
    Some(((value * 255 + (max / 2)) / max) as u8)
}

fn parse_u16_saturating(bytes: &[u8]) -> u16 {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .map(|n| n.min(u16::MAX as u32) as u16)
        .unwrap_or(0)
}

fn encode_terminal_query_response(
    query: TerminalQuery,
    cursor: (u16, u16),
    kitty_keyboard_flags: u16,
    palette: TerminalPalette,
) -> String {
    match query {
        TerminalQuery::CursorPositionReport => {
            let row = cursor.0.saturating_add(1);
            let col = cursor.1.saturating_add(1);
            format!("\x1b[{row};{col}R")
        }
        TerminalQuery::PrimaryDeviceAttributes => "\x1b[?1;2c".to_string(),
        TerminalQuery::SecondaryDeviceAttributes => "\x1b[>0;95;0c".to_string(),
        TerminalQuery::KittyKeyboardQuery => format!("\x1b[?{kitty_keyboard_flags}u"),
        TerminalQuery::ForegroundColorQuery => {
            encode_osc_color_query_response(10, palette.default_fg)
        }
        TerminalQuery::BackgroundColorQuery => {
            encode_osc_color_query_response(11, palette.default_bg)
        }
        TerminalQuery::RequestStatusStringSgr => "\x1bP1$r0m\x1b\\".to_string(),
    }
}

fn encode_osc_color_query_response(code: u8, color: Rgb) -> String {
    let r = (color.r as u16) * 0x101;
    let g = (color.g as u16) * 0x101;
    let b = (color.b as u16) * 0x101;
    format!("\x1b]{code};rgb:{r:04x}/{g:04x}/{b:04x}\x07")
}

#[cfg(test)]
mod tests {
    use super::*;
    use growterm_render_cmd::TerminalPalette;

    fn test_palette() -> TerminalPalette {
        TerminalPalette {
            default_fg: growterm_types::Rgb::new(0x12, 0x34, 0x56),
            default_bg: growterm_types::Rgb::new(0x9a, 0xbc, 0xde),
        }
    }

    #[test]
    fn new_manager_is_empty() {
        let mgr = TabManager::new();
        assert!(mgr.is_empty());
        assert_eq!(mgr.tab_count(), 0);
        assert!(mgr.active_tab().is_none());
    }

    fn dummy_tab() -> Tab {
        let grid = Grid::new(80, 24);
        let vt_parser = VtParser::new();
        let terminal = Arc::new(Mutex::new(TerminalState {
            grid,
            vt_parser,
            palette: TerminalPalette::default(),
        }));
        let dirty = Arc::new(AtomicBool::new(false));
        // We can't create a real PtyWriter without spawning, so we test TabManager logic
        // separately. For unit tests we'll test TabManager methods that don't need PtyWriter.
        // Instead, create a stub by spawning a real PTY (acceptable for unit test).
        let (_, pty_writer) = growterm_pty::spawn(24, 80).unwrap();
        Tab {
            id: 0, // assigned by TabManager::add_tab
            terminal,
            pty_writer,
            dirty,
            sync_output: Arc::new(AtomicBool::new(false)),
            last_pty_output_at: Arc::new(Mutex::new(None)),
            response_timer: ResponseTimer::new(),
            bracketed_paste: Arc::new(AtomicBool::new(false)),
            mouse_mode: Arc::new(AtomicU8::new(0)),
        }
    }

    #[test]
    fn add_tab_activates_new_tab() {
        let mut mgr = TabManager::new();
        mgr.add_tab(dummy_tab());
        assert_eq!(mgr.tab_count(), 1);
        assert_eq!(mgr.active_index(), 0);

        mgr.add_tab(dummy_tab());
        assert_eq!(mgr.tab_count(), 2);
        assert_eq!(mgr.active_index(), 1);
    }

    #[test]
    fn add_tab_inserts_after_active() {
        let mut mgr = TabManager::new();
        mgr.add_tab(dummy_tab()); // tab A at 0
        mgr.add_tab(dummy_tab()); // tab B at 1
        mgr.add_tab(dummy_tab()); // tab C at 2

        // Switch to tab 0 (A), then add a new tab
        mgr.switch_to(0);
        mgr.add_tab(dummy_tab()); // tab D should be at index 1
        assert_eq!(mgr.tab_count(), 4);
        assert_eq!(mgr.active_index(), 1); // new tab is active at index 1
    }

    #[test]
    fn switch_to_valid_index() {
        let mut mgr = TabManager::new();
        mgr.add_tab(dummy_tab());
        mgr.add_tab(dummy_tab());
        mgr.add_tab(dummy_tab());

        mgr.switch_to(0);
        assert_eq!(mgr.active_index(), 0);

        mgr.switch_to(2);
        assert_eq!(mgr.active_index(), 2);
    }

    #[test]
    fn switch_to_invalid_index_no_change() {
        let mut mgr = TabManager::new();
        mgr.add_tab(dummy_tab());
        mgr.switch_to(5);
        assert_eq!(mgr.active_index(), 0);
    }

    #[test]
    fn next_prev_tab_wraps() {
        let mut mgr = TabManager::new();
        mgr.add_tab(dummy_tab());
        mgr.add_tab(dummy_tab());
        mgr.add_tab(dummy_tab());

        mgr.switch_to(0);

        mgr.next_tab();
        assert_eq!(mgr.active_index(), 1);
        mgr.next_tab();
        assert_eq!(mgr.active_index(), 2);
        mgr.next_tab();
        assert_eq!(mgr.active_index(), 0); // wrap

        mgr.prev_tab();
        assert_eq!(mgr.active_index(), 2); // wrap back
        mgr.prev_tab();
        assert_eq!(mgr.active_index(), 1);
    }

    #[test]
    fn close_tab_adjusts_active() {
        let mut mgr = TabManager::new();
        mgr.add_tab(dummy_tab());
        mgr.add_tab(dummy_tab());
        mgr.add_tab(dummy_tab());

        // Active is 2 (last added). Close tab 1.
        let removed = mgr.close_tab(1);
        assert!(removed.is_some());
        assert_eq!(mgr.tab_count(), 2);
        // active was 2, index 1 was removed, so active adjusts to 1
        assert_eq!(mgr.active_index(), 1);
    }

    #[test]
    fn close_active_tab() {
        let mut mgr = TabManager::new();
        mgr.add_tab(dummy_tab());
        mgr.add_tab(dummy_tab());
        mgr.switch_to(0);

        let removed = mgr.close_active();
        assert!(removed.is_some());
        assert_eq!(mgr.tab_count(), 1);
        assert_eq!(mgr.active_index(), 0);
    }

    #[test]
    fn close_last_remaining_tab() {
        let mut mgr = TabManager::new();
        mgr.add_tab(dummy_tab());
        let removed = mgr.close_active();
        assert!(removed.is_some());
        assert!(mgr.is_empty());
    }

    #[test]
    fn tab_bar_info_reflects_state() {
        let mut mgr = TabManager::new();
        mgr.add_tab(dummy_tab());
        mgr.add_tab(dummy_tab());

        let info = mgr.tab_bar_info();
        assert_eq!(info.titles, vec!["⌘1", "⌘2"]);
        assert_eq!(info.active_index, 1);
    }

    #[test]
    fn extract_terminal_queries_detects_known_queries() {
        let mut pending = b"\x1b[6n\x1b[?u\x1b[c\x1b[>0c".to_vec();
        let controls = extract_terminal_controls(&mut pending);
        assert_eq!(
            controls,
            vec![
                TerminalControl::Query(TerminalQuery::CursorPositionReport),
                TerminalControl::Query(TerminalQuery::KittyKeyboardQuery),
                TerminalControl::Query(TerminalQuery::PrimaryDeviceAttributes),
                TerminalControl::Query(TerminalQuery::SecondaryDeviceAttributes),
            ]
        );
        assert!(pending.is_empty());
    }

    #[test]
    fn extract_terminal_queries_keeps_partial_sequence() {
        let mut pending = b"\x1b[6".to_vec();
        let controls = extract_terminal_controls(&mut pending);
        assert!(controls.is_empty());
        assert_eq!(pending, b"\x1b[6");
    }

    #[test]
    fn extract_terminal_queries_detects_osc_color_queries_with_bel() {
        let mut pending = b"\x1b]10;?\x07\x1b]11;?\x07".to_vec();
        let controls = extract_terminal_controls(&mut pending);
        assert_eq!(
            controls,
            vec![
                TerminalControl::Query(TerminalQuery::ForegroundColorQuery),
                TerminalControl::Query(TerminalQuery::BackgroundColorQuery)
            ]
        );
        assert!(pending.is_empty());
    }

    #[test]
    fn extract_terminal_controls_detects_kitty_push_query_pop() {
        let mut pending = b"\x1b[>7u\x1b[?u\x1b[<1u".to_vec();
        let controls = extract_terminal_controls(&mut pending);
        assert_eq!(
            controls,
            vec![
                TerminalControl::KittyKeyboardPush(7),
                TerminalControl::Query(TerminalQuery::KittyKeyboardQuery),
                TerminalControl::KittyKeyboardPop(1)
            ]
        );
        assert!(pending.is_empty());
    }

    #[test]
    fn extract_terminal_controls_detects_decrqss_sgr_query() {
        let mut pending = b"\x1bP$qm\x1b\\".to_vec();
        let controls = extract_terminal_controls(&mut pending);
        assert_eq!(
            controls,
            vec![TerminalControl::Query(
                TerminalQuery::RequestStatusStringSgr
            )]
        );
        assert!(pending.is_empty());
    }

    #[test]
    fn extract_terminal_controls_keeps_partial_kitty_push() {
        let mut pending = b"\x1b[>7".to_vec();
        let controls = extract_terminal_controls(&mut pending);
        assert!(controls.is_empty());
        assert_eq!(pending, b"\x1b[>7");
    }

    #[test]
    fn kitty_keyboard_query_response_uses_runtime_flags() {
        let response = encode_terminal_query_response(
            TerminalQuery::KittyKeyboardQuery,
            (0, 0),
            7,
            test_palette(),
        );
        assert_eq!(response, "\x1b[?7u");
    }

    #[test]
    fn osc_query_responses_use_bel_terminator() {
        let fg = encode_terminal_query_response(
            TerminalQuery::ForegroundColorQuery,
            (0, 0),
            0,
            test_palette(),
        );
        let bg = encode_terminal_query_response(
            TerminalQuery::BackgroundColorQuery,
            (0, 0),
            0,
            test_palette(),
        );
        assert_eq!(fg, "\x1b]10;rgb:1212/3434/5656\x07");
        assert_eq!(bg, "\x1b]11;rgb:9a9a/bcbc/dede\x07");
    }

    #[test]
    fn decrqss_sgr_response_is_supported() {
        let response = encode_terminal_query_response(
            TerminalQuery::RequestStatusStringSgr,
            (0, 0),
            0,
            test_palette(),
        );
        assert_eq!(response, "\x1bP1$r0m\x1b\\");
    }

    #[test]
    fn extract_terminal_controls_detects_sync_output_begin() {
        let mut pending = b"\x1b[?2026h".to_vec();
        let controls = extract_terminal_controls(&mut pending);
        assert_eq!(controls, vec![TerminalControl::SyncOutputBegin]);
        assert!(pending.is_empty());
    }

    #[test]
    fn extract_terminal_controls_detects_sync_output_end() {
        let mut pending = b"\x1b[?2026l".to_vec();
        let controls = extract_terminal_controls(&mut pending);
        assert_eq!(controls, vec![TerminalControl::SyncOutputEnd]);
        assert!(pending.is_empty());
    }

    #[test]
    fn extract_terminal_controls_keeps_partial_sync_output() {
        let mut pending = b"\x1b[?2026".to_vec();
        let controls = extract_terminal_controls(&mut pending);
        assert!(controls.is_empty());
        assert_eq!(pending, b"\x1b[?2026");
    }

    #[test]
    fn extract_terminal_controls_detects_osc_color_set_sequences() {
        let mut pending = b"\x1b]10;rgb:ffff/0000/0000\x07\x1b]11;#112233\x1b\\".to_vec();
        let controls = extract_terminal_controls(&mut pending);
        assert_eq!(
            controls,
            vec![
                TerminalControl::SetDefaultForegroundColor(growterm_types::Rgb::new(255, 0, 0)),
                TerminalControl::SetDefaultBackgroundColor(growterm_types::Rgb::new(17, 34, 51)),
            ]
        );
        assert!(pending.is_empty());
    }

    #[test]
    fn tab_index_at_x_returns_none_when_single_tab() {
        let mut mgr = TabManager::new();
        mgr.add_tab(dummy_tab());
        assert_eq!(mgr.tab_index_at_x(50.0, 800.0), None);
    }

    #[test]
    fn tab_index_at_x_returns_correct_index() {
        let mut mgr = TabManager::new();
        mgr.add_tab(dummy_tab());
        mgr.add_tab(dummy_tab());
        mgr.add_tab(dummy_tab());
        // screen_w=900, 3 tabs => each tab is 300px wide
        assert_eq!(mgr.tab_index_at_x(0.0, 900.0), Some(0));
        assert_eq!(mgr.tab_index_at_x(150.0, 900.0), Some(0));
        assert_eq!(mgr.tab_index_at_x(299.0, 900.0), Some(0));
        assert_eq!(mgr.tab_index_at_x(300.0, 900.0), Some(1));
        assert_eq!(mgr.tab_index_at_x(600.0, 900.0), Some(2));
        assert_eq!(mgr.tab_index_at_x(899.0, 900.0), Some(2));
    }

    #[test]
    fn tab_index_at_x_out_of_range() {
        let mut mgr = TabManager::new();
        mgr.add_tab(dummy_tab());
        mgr.add_tab(dummy_tab());
        assert_eq!(mgr.tab_index_at_x(800.0, 800.0), None);
    }

    #[test]
    fn mouse_y_offset_no_title_bar_no_scrollback() {
        let mut mgr = TabManager::new();
        mgr.add_tab(dummy_tab());
        assert_eq!(mgr.mouse_y_offset(20.0, 0.0, false), 0.0);
    }

    #[test]
    fn mouse_y_offset_title_bar_no_scrollback() {
        let mut mgr = TabManager::new();
        mgr.add_tab(dummy_tab());
        // transparent mode, no scrollback: includes title bar
        assert_eq!(mgr.mouse_y_offset(20.0, 50.0, false), 50.0);
    }

    #[test]
    fn mouse_y_offset_title_bar_with_scrollback() {
        let mut mgr = TabManager::new();
        mgr.add_tab(dummy_tab());
        // transparent mode, screen_full: title bar excluded (renderer uses y_off=0)
        assert_eq!(mgr.mouse_y_offset(20.0, 50.0, true), 0.0);
    }

    #[test]
    fn mouse_y_offset_tabs_plus_title_bar_no_scrollback() {
        let mut mgr = TabManager::new();
        mgr.add_tab(dummy_tab());
        mgr.add_tab(dummy_tab());
        assert_eq!(mgr.mouse_y_offset(30.0, 50.0, false), 80.0);
    }

    #[test]
    fn mouse_y_offset_tabs_with_scrollback() {
        let mut mgr = TabManager::new();
        mgr.add_tab(dummy_tab());
        mgr.add_tab(dummy_tab());
        // transparent + screen_full: renderer uses y_off=0
        assert_eq!(mgr.mouse_y_offset(30.0, 50.0, true), 0.0);
    }

    #[test]
    fn hit_test_tab_bar_normal_mode() {
        // tab_bar_h=30, content_y_off=30 → tab bar occupies [0, 30)
        assert!(hit_test_tab_bar(0.0, 30.0, 30.0));
        assert!(hit_test_tab_bar(15.0, 30.0, 30.0));
        assert!(hit_test_tab_bar(29.9, 30.0, 30.0));
        assert!(!hit_test_tab_bar(30.0, 30.0, 30.0));
    }

    #[test]
    fn hit_test_tab_bar_transparent_mode() {
        // tab_bar_h=30, content_y_off=80 → tab bar occupies [50, 80)
        assert!(!hit_test_tab_bar(0.0, 30.0, 80.0));
        assert!(!hit_test_tab_bar(49.9, 30.0, 80.0));
        assert!(hit_test_tab_bar(50.0, 30.0, 80.0));
        assert!(hit_test_tab_bar(65.0, 30.0, 80.0));
        assert!(hit_test_tab_bar(79.9, 30.0, 80.0));
        assert!(!hit_test_tab_bar(80.0, 30.0, 80.0));
    }

    #[test]
    fn hit_test_tab_bar_screen_full() {
        // content_y_off=0 → tab bar at [-30, 0), no hit possible
        assert!(!hit_test_tab_bar(0.0, 30.0, 0.0));
    }

    #[test]
    fn move_tab_forward() {
        // add_tab inserts after active: tabs=[0,1,2], active=2
        let mut mgr = TabManager::new();
        mgr.add_tab(dummy_tab()); // id=0
        mgr.add_tab(dummy_tab()); // id=1
        mgr.add_tab(dummy_tab()); // id=2
        mgr.switch_to(0); // active=0
        mgr.move_tab(0, 2);
        assert_eq!(mgr.active, 2); // active moved with tab
        assert_eq!(mgr.tabs[0].id, 1);
        assert_eq!(mgr.tabs[1].id, 2);
        assert_eq!(mgr.tabs[2].id, 0);
    }

    #[test]
    fn move_tab_backward() {
        let mut mgr = TabManager::new();
        mgr.add_tab(dummy_tab()); // id=0
        mgr.add_tab(dummy_tab()); // id=1
        mgr.add_tab(dummy_tab()); // id=2
        // tabs=[0,1,2], active=2
        mgr.move_tab(2, 0);
        assert_eq!(mgr.active, 0); // active moved with tab
        assert_eq!(mgr.tabs[0].id, 2);
        assert_eq!(mgr.tabs[1].id, 0);
        assert_eq!(mgr.tabs[2].id, 1);
    }

    #[test]
    fn move_tab_active_between_forward() {
        let mut mgr = TabManager::new();
        mgr.add_tab(dummy_tab()); // id=0
        mgr.add_tab(dummy_tab()); // id=1
        mgr.add_tab(dummy_tab()); // id=2
        // tabs=[0,1,2], active=2
        mgr.switch_to(1); // active=1
        // move tab 0 to 2: active(1) is in (from..=to] => shifted left
        mgr.move_tab(0, 2);
        assert_eq!(mgr.active, 0);
    }

    #[test]
    fn move_tab_active_between_backward() {
        let mut mgr = TabManager::new();
        mgr.add_tab(dummy_tab()); // id=0
        mgr.add_tab(dummy_tab()); // id=1
        mgr.add_tab(dummy_tab()); // id=2
        // tabs=[0,1,2], active=2
        mgr.switch_to(1); // active=1
        // move tab 2 to 0: active(1) is in [to..from) => shifted right
        mgr.move_tab(2, 0);
        assert_eq!(mgr.active, 2);
    }

    #[test]
    fn move_tab_noop_same_index() {
        let mut mgr = TabManager::new();
        mgr.add_tab(dummy_tab());
        mgr.add_tab(dummy_tab());
        mgr.move_tab(0, 0);
        assert_eq!(mgr.tabs[0].id, 0);
    }

    #[test]
    fn move_tab_out_of_bounds() {
        let mut mgr = TabManager::new();
        mgr.add_tab(dummy_tab());
        mgr.add_tab(dummy_tab());
        mgr.move_tab(0, 5); // should be noop
        assert_eq!(mgr.tabs[0].id, 0);
    }

    #[test]
    fn extract_terminal_controls_detects_bracketed_paste_enable() {
        let mut pending = b"\x1b[?2004h".to_vec();
        let controls = extract_terminal_controls(&mut pending);
        assert_eq!(controls, vec![TerminalControl::BracketedPasteEnable]);
        assert!(pending.is_empty());
    }

    #[test]
    fn extract_terminal_controls_detects_bracketed_paste_disable() {
        let mut pending = b"\x1b[?2004l".to_vec();
        let controls = extract_terminal_controls(&mut pending);
        assert_eq!(controls, vec![TerminalControl::BracketedPasteDisable]);
        assert!(pending.is_empty());
    }

    #[test]
    fn extract_terminal_controls_keeps_partial_bracketed_paste() {
        let mut pending = b"\x1b[?2004".to_vec();
        let controls = extract_terminal_controls(&mut pending);
        assert!(controls.is_empty());
        assert_eq!(pending, b"\x1b[?2004");
    }

    #[test]
    fn extract_terminal_controls_detects_mouse_normal_mode() {
        let mut pending = b"\x1b[?1000h".to_vec();
        let controls = extract_terminal_controls(&mut pending);
        assert_eq!(controls, vec![TerminalControl::MouseModeSet(1)]);
        assert!(pending.is_empty());

        let mut pending = b"\x1b[?1000l".to_vec();
        let controls = extract_terminal_controls(&mut pending);
        assert_eq!(controls, vec![TerminalControl::MouseModeSet(0)]);
        assert!(pending.is_empty());
    }

    #[test]
    fn extract_terminal_controls_detects_mouse_button_mode() {
        let mut pending = b"\x1b[?1002h".to_vec();
        let controls = extract_terminal_controls(&mut pending);
        assert_eq!(controls, vec![TerminalControl::MouseModeSet(2)]);
        assert!(pending.is_empty());

        let mut pending = b"\x1b[?1002l".to_vec();
        let controls = extract_terminal_controls(&mut pending);
        assert_eq!(controls, vec![TerminalControl::MouseModeSet(0)]);
        assert!(pending.is_empty());
    }

    #[test]
    fn extract_terminal_controls_detects_mouse_any_mode() {
        let mut pending = b"\x1b[?1003h".to_vec();
        let controls = extract_terminal_controls(&mut pending);
        assert_eq!(controls, vec![TerminalControl::MouseModeSet(3)]);
        assert!(pending.is_empty());

        let mut pending = b"\x1b[?1003l".to_vec();
        let controls = extract_terminal_controls(&mut pending);
        assert_eq!(controls, vec![TerminalControl::MouseModeSet(0)]);
        assert!(pending.is_empty());
    }

    #[test]
    fn extract_terminal_controls_detects_mouse_sgr_mode() {
        let mut pending = b"\x1b[?1006h".to_vec();
        let controls = extract_terminal_controls(&mut pending);
        assert_eq!(controls, vec![TerminalControl::MouseSgrEnable]);
        assert!(pending.is_empty());

        let mut pending = b"\x1b[?1006l".to_vec();
        let controls = extract_terminal_controls(&mut pending);
        assert_eq!(controls, vec![TerminalControl::MouseSgrDisable]);
        assert!(pending.is_empty());
    }

    #[test]
    fn extract_terminal_controls_keeps_partial_mouse_mode() {
        let mut pending = b"\x1b[?1000".to_vec();
        let controls = extract_terminal_controls(&mut pending);
        assert!(controls.is_empty());
        assert_eq!(pending, b"\x1b[?1000");

        let mut pending = b"\x1b[?100".to_vec();
        let controls = extract_terminal_controls(&mut pending);
        assert!(controls.is_empty());
        assert_eq!(pending, b"\x1b[?100");
    }

    #[test]
    fn extract_terminal_controls_detects_combined_mouse_sequences() {
        // vim typically sends: enable any-event tracking + SGR encoding
        let mut pending = b"\x1b[?1003h\x1b[?1006h".to_vec();
        let controls = extract_terminal_controls(&mut pending);
        assert_eq!(
            controls,
            vec![
                TerminalControl::MouseModeSet(3),
                TerminalControl::MouseSgrEnable,
            ]
        );
        assert!(pending.is_empty());
    }

    #[test]
    fn click_in_tab_bar_region_is_tab_bar() {
        let mut mgr = TabManager::new();
        mgr.add_tab(dummy_tab());
        mgr.add_tab(dummy_tab());
        let cell_h: f32 = 20.0;
        // y < cell_h means tab bar area
        assert!(mgr.show_tab_bar() && (10.0_f32) < cell_h);
        // y >= cell_h means terminal area
        assert!(!(mgr.show_tab_bar() && (20.0_f32) < cell_h));
        assert!(!(mgr.show_tab_bar() && (25.0_f32) < cell_h));
    }
}
