use std::cell::{Cell, RefCell};
use std::sync::mpsc::Sender;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, Sel};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSDragOperation, NSDraggingDestination, NSDraggingInfo, NSEvent, NSEventModifierFlags,
    NSTextInputClient, NSView,
};
use objc2_foundation::{
    NSArray, NSAttributedString, NSAttributedStringKey, NSCopying, NSPoint, NSRange,
    NSRangePointer, NSRect, NSString, NSUInteger,
};

use crate::event::{AppEvent, Modifiers};

/// IME 상태 머신 (WezTerm 방식)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImeState {
    /// interpretKeyEvents 호출 전 초기 상태
    None,
    /// insertText: 또는 setMarkedText: 호출됨 (IME가 처리)
    Acted,
    /// doCommandBySelector: 호출됨 (IME가 패스, 앱이 처리)
    Continue,
}

#[doc(hidden)]
pub struct Ivars {
    sender: RefCell<Option<Sender<AppEvent>>>,
    ime_state: Cell<ImeState>,
    marked_text: RefCell<String>,
    current_event: RefCell<Option<Retained<NSEvent>>>,
    pending_resize: Cell<Option<(u32, u32)>>,
    last_mouse_pos: Cell<(f64, f64)>,
    copy_mode_bypass_ime: Cell<bool>,
    /// insertText:가 조합 중인 텍스트를 확정했는지 추적
    ime_committed_from_composition: Cell<bool>,
}

define_class! {
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "GrowTerminalView"]
    #[ivars = Ivars]
    pub struct TerminalView;

    // --- NSView / NSResponder overrides ---

    impl TerminalView {
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(wantsUpdateLayer))]
        fn wants_update_layer(&self) -> bool {
            true
        }

        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(updateLayer))]
        fn update_layer(&self) {
            self.send_event(AppEvent::RedrawRequested);
        }

        /// Cmd 키 조합 (Cmd+V, Cmd+=/- 등)을 메뉴 시스템보다 먼저 가로챔.
        /// 이 메서드가 true를 반환하면 keyDown:이 호출되지 않으므로
        /// Cmd 조합은 여기서 직접 KeyInput 이벤트로 전달한다.
        #[unsafe(method(performKeyEquivalent:))]
        fn perform_key_equivalent(&self, event: &NSEvent) -> objc2::runtime::Bool {
            let flags = event.modifierFlags();
            if flags.contains(NSEventModifierFlags::Command) {
                // Cmd+Q, Cmd+P, Cmd+Shift+R은 메뉴로 처리
                let kc = event.keyCode();
                let has_shift = flags.contains(NSEventModifierFlags::Shift);
                if kc == crate::key_convert::keycode::ANSI_Q
                    || kc == crate::key_convert::keycode::ANSI_P
                    || (kc == crate::key_convert::keycode::ANSI_R && has_shift)
                {
                    return objc2::runtime::Bool::NO;
                }
                self.dispatch_key_event(event);
                return objc2::runtime::Bool::YES;
            }
            objc2::runtime::Bool::NO
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            // 복사모드: IME를 우회하고 raw keycode를 직접 전달
            if self.ivars().copy_mode_bypass_ime.get() {
                self.dispatch_key_event(event);
                return;
            }

            self.ivars().ime_state.set(ImeState::None);
            self.ivars().ime_committed_from_composition.set(false);
            self.ivars().current_event.replace(Some(event.copy()));

            // IME로 라우팅
            let events = NSArray::from_retained_slice(&[event.copy()]);
            self.interpretKeyEvents(&events);

            let state = self.ivars().ime_state.get();
            match state {
                ImeState::Acted => {
                    // IME가 처리함 (insertText 또는 setMarkedText 호출됨)
                }
                ImeState::Continue | ImeState::None => {
                    if self.ivars().ime_committed_from_composition.get() {
                        self.defer_dispatch_key_event(event);
                    } else {
                        self.dispatch_key_event(event);
                    }
                }
            }

            self.ivars().current_event.replace(None);
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            let (x, y) = self.event_location_in_backing(event);
            self.ivars().last_mouse_pos.set((x, y));
            let modifiers = convert_modifier_flags(event.modifierFlags());
            self.send_event(AppEvent::MouseMoved(x, y, modifiers));
        }

        #[unsafe(method(flagsChanged:))]
        fn flags_changed(&self, event: &NSEvent) {
            // Cmd 키 변경 시 마지막 마우스 위치로 MouseMoved 재전송
            let (x, y) = self.ivars().last_mouse_pos.get();
            let modifiers = convert_modifier_flags(event.modifierFlags());
            self.send_event(AppEvent::MouseMoved(x, y, modifiers));
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let (x, y) = self.event_location_in_backing(event);
            let modifiers = convert_modifier_flags(event.modifierFlags());
            self.send_event(AppEvent::MouseDown(x, y, modifiers));
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let (x, y) = self.event_location_in_backing(event);
            self.send_event(AppEvent::MouseDragged(x, y));
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let (x, y) = self.event_location_in_backing(event);
            self.send_event(AppEvent::MouseUp(x, y));
        }

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            let delta_y = if event.hasPreciseScrollingDeltas() {
                // 트랙패드: 픽셀 단위 → 그대로 전달 (app에서 누적)
                event.scrollingDeltaY()
            } else {
                // 마우스 휠: line 단위 → 셀 높이를 곱해서 픽셀 단위로 변환
                let scale = self.backing_scale_factor();
                event.scrollingDeltaY() * 40.0 * scale
            };
            if delta_y != 0.0 {
                self.send_event(AppEvent::ScrollWheel(delta_y));
            }
        }

        #[unsafe(method(setFrameSize:))]
        fn set_frame_size(&self, new_size: objc2_foundation::NSSize) {
            let _: () = unsafe { msg_send![super(self), setFrameSize: new_size] };
            let scale = self.backing_scale_factor();
            let w = (new_size.width * scale) as u32;
            let h = (new_size.height * scale) as u32;
            if w == 0 || h == 0 {
                return;
            }

            if self.inLiveResize() {
                // During live resize: just stash the size, let Core Animation scale
                self.ivars().pending_resize.set(Some((w, h)));
            } else {
                self.ivars().pending_resize.set(None);
                self.send_event(AppEvent::Resize(w, h));
            }
        }

        #[unsafe(method(viewDidEndLiveResize))]
        fn view_did_end_live_resize(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidEndLiveResize] };
            if let Some((w, h)) = self.ivars().pending_resize.get() {
                self.ivars().pending_resize.set(None);
                self.send_event(AppEvent::Resize(w, h));
            }
        }

        #[unsafe(method(togglePomodoro:))]
        fn toggle_pomodoro(&self, _sender: &AnyObject) {
            self.send_event(AppEvent::TogglePomodoro);
        }

        #[unsafe(method(toggleResponseTimer:))]
        fn toggle_response_timer(&self, _sender: &AnyObject) {
            self.send_event(AppEvent::ToggleResponseTimer);
        }

        #[unsafe(method(toggleCoaching:))]
        fn toggle_coaching(&self, _sender: &AnyObject) {
            self.send_event(AppEvent::ToggleCoaching);
        }

        #[unsafe(method(toggleTransparentTabBar:))]
        fn toggle_transparent_tab_bar(&self, _sender: &AnyObject) {
            self.send_event(AppEvent::ToggleTransparentTabBar);
        }

        #[unsafe(method(reloadConfig:))]
        fn reload_config(&self, _sender: &AnyObject) {
            self.send_event(AppEvent::ReloadConfig);
        }

        #[unsafe(method(viewDidChangeBackingProperties))]
        fn view_did_change_backing_properties(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeBackingProperties] };
            if let Some(layer) = self.layer() {
                layer.setContentsScale(self.backing_scale_factor());
            }
        }
    }

    unsafe impl NSObjectProtocol for TerminalView {}

    // --- NSDraggingDestination ---

    unsafe impl NSDraggingDestination for TerminalView {
        #[unsafe(method(draggingEntered:))]
        fn dragging_entered(
            &self,
            sender: &objc2::runtime::ProtocolObject<dyn NSDraggingInfo>,
        ) -> NSDragOperation {
            let pasteboard = sender.draggingPasteboard();
            if let Some(filenames) = extract_dropped_paths(&pasteboard) {
                if !filenames.is_empty() {
                    return NSDragOperation::Copy;
                }
            }
            NSDragOperation::None
        }

        #[unsafe(method(prepareForDragOperation:))]
        fn prepare_for_drag_operation(
            &self,
            _sender: &objc2::runtime::ProtocolObject<dyn NSDraggingInfo>,
        ) -> objc2::runtime::Bool {
            objc2::runtime::Bool::YES
        }

        #[unsafe(method(performDragOperation:))]
        fn perform_drag_operation(
            &self,
            sender: &objc2::runtime::ProtocolObject<dyn NSDraggingInfo>,
        ) -> objc2::runtime::Bool {
            let pasteboard = sender.draggingPasteboard();
            if let Some(paths) = extract_dropped_paths(&pasteboard) {
                if !paths.is_empty() {
                    self.send_event(AppEvent::FileDropped(paths));
                    return objc2::runtime::Bool::YES;
                }
            }
            objc2::runtime::Bool::NO
        }
    }

    // --- NSTextInputClient ---

    unsafe impl NSTextInputClient for TerminalView {
        #[unsafe(method(insertText:replacementRange:))]
        fn insert_text(&self, string: &AnyObject, _replacement_range: NSRange) {
            self.ivars().ime_state.set(ImeState::Acted);

            let text = nsobj_to_string(string);
            let was_composing;
            {
                let mut marked = self.ivars().marked_text.borrow_mut();
                was_composing = !marked.is_empty();
                if was_composing {
                    marked.clear();
                    self.send_event(AppEvent::Preedit(String::new()));
                }
            }
            self.ivars().ime_committed_from_composition.set(was_composing);
            self.send_event(AppEvent::TextCommit(text));
        }

        #[unsafe(method(doCommandBySelector:))]
        fn do_command_by_selector(&self, _selector: Sel) {
            self.ivars().ime_state.set(ImeState::Continue);
        }

        #[unsafe(method(setMarkedText:selectedRange:replacementRange:))]
        fn set_marked_text(
            &self,
            string: &AnyObject,
            _selected_range: NSRange,
            _replacement_range: NSRange,
        ) {
            self.ivars().ime_state.set(ImeState::Acted);

            let text = nsobj_to_string(string);
            self.ivars().marked_text.replace(text.clone());
            self.send_event(AppEvent::Preedit(text));
        }

        #[unsafe(method(unmarkText))]
        fn unmark_text(&self) {
            self.ivars().marked_text.replace(String::new());
            self.send_event(AppEvent::Preedit(String::new()));
        }

        #[unsafe(method(hasMarkedText))]
        fn has_marked_text(&self) -> bool {
            !self.ivars().marked_text.borrow().is_empty()
        }

        #[unsafe(method(markedRange))]
        fn marked_range(&self) -> NSRange {
            let marked = self.ivars().marked_text.borrow();
            if marked.is_empty() {
                NSRange::new(NSUInteger::MAX, 0)
            } else {
                NSRange::new(0, marked.len())
            }
        }

        #[unsafe(method(selectedRange))]
        fn selected_range(&self) -> NSRange {
            NSRange::new(NSUInteger::MAX, 0)
        }

        #[unsafe(method_id(attributedSubstringForProposedRange:actualRange:))]
        fn attributed_substring(
            &self,
            _range: NSRange,
            _actual_range: NSRangePointer,
        ) -> Option<Retained<NSAttributedString>> {
            None
        }

        #[unsafe(method(firstRectForCharacterRange:actualRange:))]
        fn first_rect(
            &self,
            _range: NSRange,
            _actual_range: NSRangePointer,
        ) -> NSRect {
            if let Some(window) = self.window() {
                let frame = window.frame();
                NSRect::new(
                    NSPoint::new(frame.origin.x, frame.origin.y),
                    objc2_foundation::NSSize::new(0.0, 0.0),
                )
            } else {
                NSRect::ZERO
            }
        }

        #[unsafe(method(characterIndexForPoint:))]
        fn character_index_for_point(&self, _point: NSPoint) -> NSUInteger {
            0
        }

        #[unsafe(method_id(validAttributesForMarkedText))]
        fn valid_attributes(&self) -> Retained<NSArray<NSAttributedStringKey>> {
            NSArray::new()
        }
    }
}

impl TerminalView {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>();
        let this = this.set_ivars(Ivars {
            sender: RefCell::new(None),
            ime_state: Cell::new(ImeState::None),
            marked_text: RefCell::new(String::new()),
            current_event: RefCell::new(None),
            pending_resize: Cell::new(None),
            last_mouse_pos: Cell::new((0.0, 0.0)),
            copy_mode_bypass_ime: Cell::new(false),
            ime_committed_from_composition: Cell::new(false),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setWantsLayer(true);
        if let Some(layer) = this.layer() {
            layer.setContentsScale(this.backing_scale_factor());
        }
        // Register NSTrackingArea for mouseMoved events
        {
            use objc2::AnyThread;
            use objc2_app_kit::{NSTrackingArea, NSTrackingAreaOptions};
            let options = NSTrackingAreaOptions::MouseMoved
                | NSTrackingAreaOptions::ActiveInKeyWindow
                | NSTrackingAreaOptions::InVisibleRect;
            let tracking_area = unsafe {
                NSTrackingArea::initWithRect_options_owner_userInfo(
                    NSTrackingArea::alloc(),
                    NSRect::ZERO,
                    options,
                    Some(&this),
                    None,
                )
            };
            this.addTrackingArea(&tracking_area);
        }
        // Register for file drag & drop
        let file_url_type = unsafe { objc2_app_kit::NSPasteboardTypeFileURL };
        let types = NSArray::from_retained_slice(&[file_url_type.copy()]);
        this.registerForDraggedTypes(&types);
        this
    }

    pub(crate) fn set_sender(&self, sender: Sender<AppEvent>) {
        self.ivars().sender.replace(Some(sender));
    }

    pub(crate) fn set_copy_mode_bypass_ime(&self, enabled: bool) {
        self.ivars().copy_mode_bypass_ime.set(enabled);
    }

    fn send_event(&self, event: AppEvent) {
        if let Some(ref sender) = *self.ivars().sender.borrow() {
            let _ = sender.send(event);
        }
    }

    fn dispatch_key_event(&self, event: &NSEvent) {
        let keycode = event.keyCode();
        let flags = event.modifierFlags();
        let characters = event
            .charactersIgnoringModifiers()
            .map(|s| s.to_string());
        let modifiers = convert_modifier_flags(flags);
        self.send_event(AppEvent::KeyInput {
            keycode,
            characters,
            modifiers,
        });
    }

    /// 키 이벤트를 별도 스레드에서 전송 (IME 조합 확정 후 PTY에 시간차를 줌)
    fn defer_dispatch_key_event(&self, event: &NSEvent) {
        let keycode = event.keyCode();
        let flags = event.modifierFlags();
        let characters = event
            .charactersIgnoringModifiers()
            .map(|s| s.to_string());
        let modifiers = convert_modifier_flags(flags);
        let sender = self.ivars().sender.borrow().clone();
        std::thread::spawn(move || {
            // TextCommit이 PTY에 쓰이고 상대편이 읽을 시간 확보
            std::thread::sleep(std::time::Duration::from_millis(100));
            if let Some(ref sender) = sender {
                let _ = sender.send(AppEvent::KeyInput {
                    keycode,
                    characters,
                    modifiers,
                });
            }
        });
    }

    fn event_location_in_backing(&self, event: &NSEvent) -> (f64, f64) {
        let loc = event.locationInWindow();
        let local = self.convertPoint_fromView(loc, None);
        let scale = self.backing_scale_factor();
        // NSView is flipped (isFlipped returns true), so y is already top-down
        (local.x * scale, local.y * scale)
    }

    fn backing_scale_factor(&self) -> f64 {
        self.window()
            .map(|w| w.backingScaleFactor())
            .unwrap_or(2.0)
    }
}

fn nsobj_to_string(obj: &AnyObject) -> String {
    let class_name = obj.class().name().to_str().unwrap_or("");
    if class_name.contains("AttributedString") {
        let attr_str: &NSAttributedString =
            unsafe { &*(obj as *const AnyObject as *const NSAttributedString) };
        attr_str.string().to_string()
    } else {
        let ns_str: &NSString = unsafe { &*(obj as *const AnyObject as *const NSString) };
        ns_str.to_string()
    }
}

fn url_string_to_path(url_str: &str) -> String {
    // file:///path/to/file → /path/to/file (with percent-decoding)
    if let Some(path) = url_str.strip_prefix("file://") {
        let path = path.strip_prefix("localhost").unwrap_or(path);
        percent_decode(path)
    } else {
        url_str.to_string()
    }
}

fn extract_dropped_paths(pasteboard: &objc2_app_kit::NSPasteboard) -> Option<Vec<String>> {
    let file_url_type = unsafe { objc2_app_kit::NSPasteboardTypeFileURL };

    if let Some(items) = pasteboard.pasteboardItems() {
        let paths: Vec<String> = items
            .iter()
            .filter_map(|item| item.stringForType(file_url_type))
            .map(|url| url_string_to_path(&url.to_string()))
            .filter(|s| !s.is_empty())
            .collect();
        if !paths.is_empty() {
            return Some(paths);
        }
    }

    pasteboard
        .stringForType(file_url_type)
        .map(|urls_str| parse_file_url_lines(&urls_str.to_string()))
}

fn parse_file_url_lines(input: &str) -> Vec<String> {
    input
        .lines()
        .map(url_string_to_path)
        .filter(|s| !s.is_empty())
        .collect()
}

fn percent_decode(s: &str) -> String {
    let mut result = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(
                &s[i + 1..i + 3],
                16,
            ) {
                result.push(byte);
                i += 3;
                continue;
            }
        }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(result).unwrap_or_else(|_| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_string_to_path_basic() {
        assert_eq!(
            url_string_to_path("file:///Users/me/file.txt"),
            "/Users/me/file.txt"
        );
    }

    #[test]
    fn url_string_to_path_with_spaces() {
        assert_eq!(
            url_string_to_path("file:///Users/me/my%20file.txt"),
            "/Users/me/my file.txt"
        );
    }

    #[test]
    fn url_string_to_path_korean() {
        assert_eq!(
            url_string_to_path("file:///Users/me/%ED%95%9C%EA%B8%80.txt"),
            "/Users/me/한글.txt"
        );
    }

    #[test]
    fn url_string_to_path_non_file_url() {
        assert_eq!(url_string_to_path("/plain/path"), "/plain/path");
    }

    #[test]
    fn url_string_to_path_localhost_url() {
        assert_eq!(
            url_string_to_path("file://localhost/Users/me/file.txt"),
            "/Users/me/file.txt"
        );
    }

    #[test]
    fn parse_file_url_lines_multiple_urls() {
        let input = "file:///tmp/a.png\nfile:///tmp/b%20c.jpg";
        assert_eq!(
            parse_file_url_lines(input),
            vec!["/tmp/a.png".to_string(), "/tmp/b c.jpg".to_string()]
        );
    }

    #[test]
    fn parse_file_url_lines_ignores_empty_lines() {
        let input = "\nfile:///tmp/a.png\n\n";
        assert_eq!(
            parse_file_url_lines(input),
            vec!["/tmp/a.png".to_string()]
        );
    }
}

fn convert_modifier_flags(flags: NSEventModifierFlags) -> Modifiers {
    let mut mods = Modifiers::empty();
    if flags.contains(NSEventModifierFlags::Shift) {
        mods |= Modifiers::SHIFT;
    }
    if flags.contains(NSEventModifierFlags::Control) {
        mods |= Modifiers::CONTROL;
    }
    if flags.contains(NSEventModifierFlags::Option) {
        mods |= Modifiers::ALT;
    }
    if flags.contains(NSEventModifierFlags::Command) {
        mods |= Modifiers::SUPER;
    }
    mods
}
