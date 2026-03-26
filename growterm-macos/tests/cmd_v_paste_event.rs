//! Integration test: Cmd+V, Cmd+=, Cmd+- 가 performKeyEquivalent를 통해
//! AppEvent::KeyInput으로 전달되는지 검증.
//!
//! 한글 IME 활성 시에도 keycode 기반으로 동작해야 한다.
//! Cmd+Q는 메뉴(terminate:)로 넘겨야 하므로 가로채지 않아야 한다.

use std::sync::mpsc;

use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSEvent, NSEventModifierFlags, NSEventType};
use objc2_foundation::NSPoint;

fn main() {
    let mtm = MainThreadMarker::new().expect("must run on main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);

    test_cmd_v_english(mtm);
    test_cmd_v_korean_ime(mtm);
    test_cmd_q_not_intercepted_english(mtm);
    test_cmd_q_not_intercepted_korean_ime(mtm);
    test_key_down_repeat_is_reported(mtm);
    test_key_up_is_reported(mtm);

    println!("PASS: all Cmd key equivalent tests passed");
}

/// Cmd+V (영문) → KeyInput 이벤트로 전달
fn test_cmd_v_english(mtm: MainThreadMarker) {
    let (view, rx) = setup_view(mtm);
    let event = create_key_event(0x09, "v", NSEventModifierFlags::Command);

    let handled = send_perform_key_equivalent(&view, &event);
    assert!(handled, "Cmd+V (english) should be handled");

    let app_event = rx.try_recv().expect("should receive KeyInput for Cmd+V");
    assert_key_input(&app_event, 0x09, growterm_macos::Modifiers::SUPER);
    println!("  PASS: Cmd+V (english)");
}

/// Cmd+V (한글 IME) → charactersIgnoringModifiers가 한글이어도 keycode로 동작
fn test_cmd_v_korean_ime(mtm: MainThreadMarker) {
    let (view, rx) = setup_view(mtm);
    // 한글 IME에서 Cmd+V → charactersIgnoringModifiers가 "ㅍ" 반환
    let event = create_key_event(0x09, "ㅍ", NSEventModifierFlags::Command);

    let handled = send_perform_key_equivalent(&view, &event);
    assert!(handled, "Cmd+V (korean IME) should be handled");

    let app_event = rx.try_recv().expect("should receive KeyInput for Cmd+V (korean)");
    assert_key_input(&app_event, 0x09, growterm_macos::Modifiers::SUPER);
    println!("  PASS: Cmd+V (korean IME)");
}

/// Cmd+Q (영문) → 메뉴로 통과 (가로채지 않음)
fn test_cmd_q_not_intercepted_english(mtm: MainThreadMarker) {
    let (view, _rx) = setup_view(mtm);
    let event = create_key_event(0x0C, "q", NSEventModifierFlags::Command);

    let handled = send_perform_key_equivalent(&view, &event);
    assert!(!handled, "Cmd+Q (english) should NOT be handled");
    println!("  PASS: Cmd+Q (english) not intercepted");
}

/// Cmd+Q (한글 IME) → 메뉴로 통과 (가로채지 않음)
fn test_cmd_q_not_intercepted_korean_ime(mtm: MainThreadMarker) {
    let (view, _rx) = setup_view(mtm);
    // 한글 IME에서 Cmd+Q → charactersIgnoringModifiers가 "ㅂ" 반환
    let event = create_key_event(0x0C, "ㅂ", NSEventModifierFlags::Command);

    let handled = send_perform_key_equivalent(&view, &event);
    assert!(!handled, "Cmd+Q (korean IME) should NOT be handled");
    println!("  PASS: Cmd+Q (korean IME) not intercepted");
}

// --- helpers ---

fn setup_view(mtm: MainThreadMarker) -> (objc2::rc::Retained<growterm_macos::view::TerminalView>, mpsc::Receiver<growterm_macos::AppEvent>) {
    let view = growterm_macos::test_support::create_terminal_view(mtm);
    let (tx, rx) = mpsc::channel();
    growterm_macos::test_support::set_view_sender(&view, tx);
    (view, rx)
}

fn send_perform_key_equivalent(view: &growterm_macos::view::TerminalView, event: &NSEvent) -> bool {
    let result: bool = unsafe {
        objc2::msg_send![view, performKeyEquivalent: event]
    };
    result
}

fn assert_key_input(event: &growterm_macos::AppEvent, expected_keycode: u16, expected_mod: growterm_macos::Modifiers) {
    match event {
        growterm_macos::AppEvent::KeyInput { keycode, modifiers, .. } => {
            assert_eq!(*keycode, expected_keycode, "keycode mismatch");
            assert!(modifiers.contains(expected_mod), "modifiers should contain {:?}", expected_mod);
        }
        other => panic!("expected KeyInput, got {:?}", other),
    }
}

fn test_key_down_repeat_is_reported(mtm: MainThreadMarker) {
    let (view, rx) = setup_view(mtm);
    growterm_macos::test_support::set_copy_mode_bypass_ime(&view, true);
    let event = create_key_event_with_type(
        NSEventType::KeyDown,
        0x7E,
        "",
        NSEventModifierFlags::empty(),
        true,
    );

    unsafe { let _: () = objc2::msg_send![&*view, keyDown: &*event]; }

    match rx.try_recv().expect("should receive repeated key input") {
        growterm_macos::AppEvent::KeyInput { keycode, event_type, .. } => {
            assert_eq!(keycode, 0x7E);
            assert_eq!(event_type, growterm_macos::KeyEventType::Repeat);
        }
        other => panic!("expected KeyInput, got {:?}", other),
    }
    println!("  PASS: keyDown repeat reported");
}

fn test_key_up_is_reported(mtm: MainThreadMarker) {
    let (view, rx) = setup_view(mtm);
    growterm_macos::test_support::set_copy_mode_bypass_ime(&view, true);
    let event = create_key_event_with_type(
        NSEventType::KeyUp,
        0x7E,
        "",
        NSEventModifierFlags::empty(),
        false,
    );

    unsafe { let _: () = objc2::msg_send![&*view, keyUp: &*event]; }

    match rx.try_recv().expect("should receive key release input") {
        growterm_macos::AppEvent::KeyInput { keycode, event_type, .. } => {
            assert_eq!(keycode, 0x7E);
            assert_eq!(event_type, growterm_macos::KeyEventType::Release);
        }
        other => panic!("expected KeyInput, got {:?}", other),
    }
    println!("  PASS: keyUp release reported");
}

fn create_key_event(keycode: u16, chars: &str, flags: NSEventModifierFlags) -> Retained<NSEvent> {
    create_key_event_with_type(NSEventType::KeyDown, keycode, chars, flags, false)
}

fn create_key_event_with_type(
    event_type: NSEventType,
    keycode: u16,
    chars: &str,
    flags: NSEventModifierFlags,
    is_repeat: bool,
) -> Retained<NSEvent> {
    use objc2_foundation::NSString;
    let characters = NSString::from_str(chars);
    let characters_ignoring = NSString::from_str(chars);
    NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
        event_type,
        NSPoint::new(0.0, 0.0),
        flags,
        0.0,
        0,
        None,
        &characters,
        &characters_ignoring,
        is_repeat,
        keycode,
    ).expect("failed to create key event")
}
