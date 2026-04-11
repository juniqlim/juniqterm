use growterm_types::{Key, KeyEvent, KeyEventType, Modifiers};

pub const KITTY_KEYBOARD_DISAMBIGUATE_ESCAPES: u16 = 0b1;
pub const KITTY_KEYBOARD_REPORT_EVENT_TYPES: u16 = 0b10;
pub const KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESCAPES: u16 = 0b1000;

/// Convert a KeyEvent to the byte sequence a terminal PTY expects.
pub fn encode(event: KeyEvent) -> Vec<u8> {
    encode_with_kitty_flags_and_event_type(event, 0, KeyEventType::Press)
}

/// Convert a KeyEvent using the negotiated kitty keyboard protocol flags.
pub fn encode_with_kitty_flags(event: KeyEvent, kitty_flags: u16) -> Vec<u8> {
    encode_with_kitty_flags_and_event_type(event, kitty_flags, KeyEventType::Press)
}

pub fn encode_with_kitty_flags_and_event_type(
    event: KeyEvent,
    kitty_flags: u16,
    event_type: KeyEventType,
) -> Vec<u8> {
    let has_alt = event.modifiers.contains(Modifiers::ALT);
    let has_ctrl = event.modifiers.contains(Modifiers::CTRL);
    let has_shift = event.modifiers.contains(Modifiers::SHIFT);
    let report_all = kitty_flags & KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESCAPES != 0;
    let disambiguate = kitty_flags & KITTY_KEYBOARD_DISAMBIGUATE_ESCAPES != 0;
    let kitty_event_type = kitty_event_type(event_type, kitty_flags);

    if event_type == KeyEventType::Release && kitty_event_type.is_none() {
        return Vec::new();
    }

    match event.key {
        Key::Enter if report_all => encode_kitty_key(13, event.modifiers, kitty_event_type),
        Key::Char(' ') if has_ctrl => {
            if kitty_event_type.is_some() {
                return encode_kitty_key(32, event.modifiers, kitty_event_type);
            }
            if has_alt {
                vec![0x1b, 0x00]
            } else {
                vec![0x00]
            }
        }
        Key::Char(c) if has_ctrl && c.is_ascii_alphabetic() => {
            if disambiguate || has_shift {
                return encode_kitty_text_key(c, event.modifiers, kitty_event_type);
            }
            // Ctrl+A = 0x01, Ctrl+Z = 0x1A
            let ctrl_byte = (c.to_ascii_lowercase() as u8) - b'a' + 1;
            if has_alt {
                vec![0x1b, ctrl_byte]
            } else {
                vec![ctrl_byte]
            }
        }
        Key::Char(c) if should_encode_text_key_as_kitty(c, event.modifiers, kitty_flags) => {
            encode_kitty_text_key(c, event.modifiers, kitty_event_type)
        }
        Key::Char(c) => {
            if event_type == KeyEventType::Release {
                return Vec::new();
            }
            let mut buf = [0u8; 4];
            let s = c.encode_utf8(&mut buf);
            if has_alt {
                let mut v = vec![0x1b];
                v.extend_from_slice(s.as_bytes());
                v
            } else {
                s.as_bytes().to_vec()
            }
        }
        Key::Enter if has_shift => encode_kitty_key(13, event.modifiers, kitty_event_type),
        Key::Enter => vec![b'\r'],
        Key::Tab if report_all => encode_kitty_key(9, event.modifiers, kitty_event_type),
        Key::Tab if kitty_event_type.is_some() => encode_kitty_key(9, event.modifiers, kitty_event_type),
        Key::Tab if has_shift && !has_alt && !has_ctrl => b"\x1b[Z".to_vec(),
        Key::Tab => vec![b'\t'],
        Key::Escape if report_all || disambiguate || !event.modifiers.is_empty() => {
            encode_kitty_key(27, event.modifiers, kitty_event_type)
        }
        Key::Escape => vec![0x1b],
        Key::Backspace if report_all => encode_kitty_key(127, event.modifiers, kitty_event_type),
        Key::Backspace if kitty_event_type.is_some() => {
            encode_kitty_key(127, event.modifiers, kitty_event_type)
        }
        Key::Backspace if has_alt => vec![0x1b, 0x7f],
        Key::Backspace => vec![0x7f],
        Key::F1 => encode_function_key(FunctionKey::F1, event.modifiers, kitty_event_type),
        Key::F2 => encode_function_key(FunctionKey::F2, event.modifiers, kitty_event_type),
        Key::F3 => encode_function_key(FunctionKey::F3, event.modifiers, kitty_event_type),
        Key::F4 => encode_function_key(FunctionKey::F4, event.modifiers, kitty_event_type),
        Key::F5 => encode_function_key(FunctionKey::F5, event.modifiers, kitty_event_type),
        Key::F6 => encode_function_key(FunctionKey::F6, event.modifiers, kitty_event_type),
        Key::F7 => encode_function_key(FunctionKey::F7, event.modifiers, kitty_event_type),
        Key::F8 => encode_function_key(FunctionKey::F8, event.modifiers, kitty_event_type),
        Key::F9 => encode_function_key(FunctionKey::F9, event.modifiers, kitty_event_type),
        Key::F10 => encode_function_key(FunctionKey::F10, event.modifiers, kitty_event_type),
        Key::F11 => encode_function_key(FunctionKey::F11, event.modifiers, kitty_event_type),
        Key::F12 => encode_function_key(FunctionKey::F12, event.modifiers, kitty_event_type),
        Key::Delete => encode_tilde(3, has_shift, has_alt, has_ctrl, kitty_event_type),
        Key::ArrowUp => encode_cursor(b'A', has_shift, has_alt, has_ctrl, kitty_event_type),
        Key::ArrowDown => encode_cursor(b'B', has_shift, has_alt, has_ctrl, kitty_event_type),
        Key::ArrowRight => encode_cursor(b'C', has_shift, has_alt, has_ctrl, kitty_event_type),
        Key::ArrowLeft => encode_cursor(b'D', has_shift, has_alt, has_ctrl, kitty_event_type),
        Key::Home => encode_cursor(b'H', has_shift, has_alt, has_ctrl, kitty_event_type),
        Key::End => encode_cursor(b'F', has_shift, has_alt, has_ctrl, kitty_event_type),
        Key::PageUp => encode_tilde(5, has_shift, has_alt, has_ctrl, kitty_event_type),
        Key::PageDown => encode_tilde(6, has_shift, has_alt, has_ctrl, kitty_event_type),
    }
}

fn should_encode_text_key_as_kitty(c: char, modifiers: Modifiers, kitty_flags: u16) -> bool {
    if kitty_flags & KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESCAPES != 0 {
        return true;
    }
    if kitty_flags & KITTY_KEYBOARD_DISAMBIGUATE_ESCAPES == 0 {
        return false;
    }
    if !is_legacy_text_key(c) {
        return false;
    }
    let has_alt = modifiers.contains(Modifiers::ALT);
    let has_ctrl = modifiers.contains(Modifiers::CTRL);
    let has_shift = modifiers.contains(Modifiers::SHIFT);
    has_ctrl || has_alt || (has_shift && has_alt)
}

fn is_legacy_text_key(c: char) -> bool {
    matches!(
        c,
        'a'..='z'
            | 'A'..='Z'
            | '0'..='9'
            | '`'
            | '~'
            | '-'
            | '_'
            | '='
            | '+'
            | '['
            | '{'
            | ']'
            | '}'
            | '\\'
            | '|'
            | ';'
            | ':'
            | '\''
            | '"'
            | ','
            | '<'
            | '.'
            | '>'
            | '/'
            | '?'
    )
}

fn encode_kitty_text_key(c: char, modifiers: Modifiers, event_type: Option<u8>) -> Vec<u8> {
    encode_kitty_key(kitty_base_key_code(c), modifiers, event_type)
}

fn kitty_base_key_code(c: char) -> u32 {
    if c.is_ascii_alphabetic() {
        return c.to_ascii_lowercase() as u32;
    }
    match c {
        '!' => '1' as u32,
        '@' => '2' as u32,
        '#' => '3' as u32,
        '$' => '4' as u32,
        '%' => '5' as u32,
        '^' => '6' as u32,
        '&' => '7' as u32,
        '*' => '8' as u32,
        '(' => '9' as u32,
        ')' => '0' as u32,
        '_' => '-' as u32,
        '+' => '=' as u32,
        '{' => '[' as u32,
        '}' => ']' as u32,
        '|' => '\\' as u32,
        ':' => ';' as u32,
        '"' => '\'' as u32,
        '<' => ',' as u32,
        '>' => '.' as u32,
        '?' => '/' as u32,
        '~' => '`' as u32,
        _ => c as u32,
    }
}

fn encode_kitty_key(codepoint: u32, modifiers: Modifiers, event_type: Option<u8>) -> Vec<u8> {
    let mut seq = format!("\x1b[{codepoint};{}", kitty_modifier_param(modifiers));
    if let Some(event_type) = event_type {
        seq.push(':');
        seq.push((b'0' + event_type) as char);
    }
    seq.push('u');
    seq.into_bytes()
}

fn kitty_modifier_param(modifiers: Modifiers) -> u8 {
    1 + (modifiers.contains(Modifiers::SHIFT) as u8)
        + (modifiers.contains(Modifiers::ALT) as u8) * 2
        + (modifiers.contains(Modifiers::CTRL) as u8) * 4
}

#[derive(Clone, Copy)]
enum FunctionKey {
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
}

fn encode_function_key(key: FunctionKey, modifiers: Modifiers, event_type: Option<u8>) -> Vec<u8> {
    let has_modifiers = !modifiers.is_empty();
    match key {
        FunctionKey::F1 => encode_ss3_or_csi_param(b'P', modifiers, has_modifiers, event_type),
        FunctionKey::F2 => encode_ss3_or_csi_param(b'Q', modifiers, has_modifiers, event_type),
        FunctionKey::F3 => encode_ss3_or_csi_param(b'R', modifiers, has_modifiers, event_type),
        FunctionKey::F4 => encode_ss3_or_csi_param(b'S', modifiers, has_modifiers, event_type),
        FunctionKey::F5 => encode_tilde_param(15, modifiers, event_type),
        FunctionKey::F6 => encode_tilde_param(17, modifiers, event_type),
        FunctionKey::F7 => encode_tilde_param(18, modifiers, event_type),
        FunctionKey::F8 => encode_tilde_param(19, modifiers, event_type),
        FunctionKey::F9 => encode_tilde_param(20, modifiers, event_type),
        FunctionKey::F10 => encode_tilde_param(21, modifiers, event_type),
        FunctionKey::F11 => encode_tilde_param(23, modifiers, event_type),
        FunctionKey::F12 => encode_tilde_param(24, modifiers, event_type),
    }
}

fn encode_ss3_or_csi_param(
    final_byte: u8,
    modifiers: Modifiers,
    force_csi: bool,
    event_type: Option<u8>,
) -> Vec<u8> {
    if !force_csi && event_type.is_none() {
        return vec![0x1b, b'O', final_byte];
    }
    let mut v = vec![0x1b, b'[', b'1', b';'];
    v.push(b'0' + kitty_modifier_param(modifiers));
    if let Some(event_type) = event_type {
        v.push(b':');
        v.push(b'0' + event_type);
    }
    v.push(final_byte);
    v
}

fn encode_tilde_param(n: u8, modifiers: Modifiers, event_type: Option<u8>) -> Vec<u8> {
    let mut v = vec![0x1b, b'['];
    v.extend_from_slice(n.to_string().as_bytes());
    if modifiers.is_empty() && event_type.is_none() {
        v.push(b'~');
        return v;
    }
    v.push(b';');
    v.push(b'0' + if modifiers.is_empty() { 1 } else { kitty_modifier_param(modifiers) });
    if let Some(event_type) = event_type {
        v.push(b':');
        v.push(b'0' + event_type);
    }
    v.push(b'~');
    v
}

/// Modifier parameter for xterm-style sequences: CSI 1;{mod} {letter}
fn modifier_param(shift: bool, alt: bool, ctrl: bool) -> Option<u8> {
    let n = 1 + (shift as u8) + (alt as u8) * 2 + (ctrl as u8) * 4;
    if n > 1 { Some(n) } else { None }
}

/// Encode cursor-key style sequences: \x1b[A or \x1b[1;{mod}A
fn encode_cursor(
    letter: u8,
    shift: bool,
    alt: bool,
    ctrl: bool,
    event_type: Option<u8>,
) -> Vec<u8> {
    match modifier_param(shift, alt, ctrl) {
        Some(m) => {
            let mut v = vec![0x1b, b'[', b'1', b';'];
            v.push(b'0' + m);
            if let Some(event_type) = event_type {
                v.push(b':');
                v.push(b'0' + event_type);
            }
            v.push(letter);
            v
        }
        None => {
            if let Some(event_type) = event_type {
                let mut v = vec![0x1b, b'[', b'1', b';', b'1', b':', b'0' + event_type];
                v.push(letter);
                v
            } else {
                vec![0x1b, b'[', letter]
            }
        }
    }
}

/// Encode tilde-style sequences: \x1b[{n}~ or \x1b[{n};{mod}~
fn encode_tilde(n: u8, shift: bool, alt: bool, ctrl: bool, event_type: Option<u8>) -> Vec<u8> {
    match modifier_param(shift, alt, ctrl) {
        Some(m) => encode_tilde_number(n, m, event_type),
        None => {
            if let Some(event_type) = event_type {
                encode_tilde_number(n, 1, Some(event_type))
            } else {
                vec![0x1b, b'[', b'0' + n, b'~']
            }
        }
    }
}

fn encode_tilde_number(n: u8, modifier: u8, event_type: Option<u8>) -> Vec<u8> {
    let mut v = vec![0x1b, b'['];
    v.extend_from_slice(n.to_string().as_bytes());
    if modifier == 1 && event_type.is_none() {
        v.push(b'~');
        return v;
    }
    v.push(b';');
    v.push(b'0' + modifier);
    if let Some(event_type) = event_type {
        v.push(b':');
        v.push(b'0' + event_type);
    }
    v.push(b'~');
    v
}

fn kitty_event_type(event_type: KeyEventType, kitty_flags: u16) -> Option<u8> {
    if kitty_flags & KITTY_KEYBOARD_REPORT_EVENT_TYPES == 0 {
        return None;
    }
    match event_type {
        KeyEventType::Press => None,
        KeyEventType::Repeat => Some(2),
        KeyEventType::Release => Some(3),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Plain characters ---

    #[test]
    fn ascii_char() {
        let event = KeyEvent { key: Key::Char('a'), modifiers: Modifiers::empty() };
        assert_eq!(encode(event), b"a");
    }

    #[test]
    fn uppercase_char() {
        let event = KeyEvent { key: Key::Char('A'), modifiers: Modifiers::empty() };
        assert_eq!(encode(event), b"A");
    }

    #[test]
    fn unicode_char() {
        let event = KeyEvent { key: Key::Char('한'), modifiers: Modifiers::empty() };
        assert_eq!(encode(event), "한".as_bytes());
    }

    #[test]
    fn space() {
        let event = KeyEvent { key: Key::Char(' '), modifiers: Modifiers::empty() };
        assert_eq!(encode(event), b" ");
    }

    #[test]
    fn ctrl_space_is_nul() {
        let event = KeyEvent { key: Key::Char(' '), modifiers: Modifiers::CTRL };
        assert_eq!(encode(event), b"\0");
    }

    #[test]
    fn ctrl_alt_space_is_esc_nul() {
        let event = KeyEvent { key: Key::Char(' '), modifiers: Modifiers::CTRL | Modifiers::ALT };
        assert_eq!(encode(event), b"\x1b\0");
    }

    // --- Special keys ---

    #[test]
    fn enter() {
        let event = KeyEvent { key: Key::Enter, modifiers: Modifiers::empty() };
        assert_eq!(encode(event), b"\r");
    }

    #[test]
    fn shift_enter() {
        let event = KeyEvent { key: Key::Enter, modifiers: Modifiers::SHIFT };
        assert_eq!(encode(event), b"\x1b[13;2u");
    }

    #[test]
    fn tab() {
        let event = KeyEvent { key: Key::Tab, modifiers: Modifiers::empty() };
        assert_eq!(encode(event), b"\t");
    }

    #[test]
    fn shift_tab() {
        let event = KeyEvent { key: Key::Tab, modifiers: Modifiers::SHIFT };
        assert_eq!(encode(event), b"\x1b[Z");
    }

    #[test]
    fn escape() {
        let event = KeyEvent { key: Key::Escape, modifiers: Modifiers::empty() };
        assert_eq!(encode(event), b"\x1b");
    }

    #[test]
    fn backspace() {
        let event = KeyEvent { key: Key::Backspace, modifiers: Modifiers::empty() };
        assert_eq!(encode(event), b"\x7f");
    }

    #[test]
    fn backspace_repeat_without_kitty() {
        let event = KeyEvent { key: Key::Backspace, modifiers: Modifiers::empty() };
        let result = encode_with_kitty_flags_and_event_type(event, 0, KeyEventType::Repeat);
        assert_eq!(result, b"\x7f", "Repeat should produce same output as Press");
    }

    #[test]
    fn char_repeat_without_kitty() {
        let event = KeyEvent { key: Key::Char('a'), modifiers: Modifiers::empty() };
        let result = encode_with_kitty_flags_and_event_type(event, 0, KeyEventType::Repeat);
        assert_eq!(result, b"a", "Repeat should produce same output as Press");
    }

    #[test]
    fn delete() {
        let event = KeyEvent { key: Key::Delete, modifiers: Modifiers::empty() };
        assert_eq!(encode(event), b"\x1b[3~");
    }

    #[test]
    fn f1() {
        let event = KeyEvent { key: Key::F1, modifiers: Modifiers::empty() };
        assert_eq!(encode(event), b"\x1bOP");
    }

    #[test]
    fn shift_f1() {
        let event = KeyEvent { key: Key::F1, modifiers: Modifiers::SHIFT };
        assert_eq!(encode(event), b"\x1b[1;2P");
    }

    #[test]
    fn f5() {
        let event = KeyEvent { key: Key::F5, modifiers: Modifiers::empty() };
        assert_eq!(encode(event), b"\x1b[15~");
    }

    #[test]
    fn ctrl_f12() {
        let event = KeyEvent { key: Key::F12, modifiers: Modifiers::CTRL };
        assert_eq!(encode(event), b"\x1b[24;5~");
    }

    // --- Arrow keys ---

    #[test]
    fn arrow_up() {
        let event = KeyEvent { key: Key::ArrowUp, modifiers: Modifiers::empty() };
        assert_eq!(encode(event), b"\x1b[A");
    }

    #[test]
    fn arrow_down() {
        let event = KeyEvent { key: Key::ArrowDown, modifiers: Modifiers::empty() };
        assert_eq!(encode(event), b"\x1b[B");
    }

    #[test]
    fn arrow_right() {
        let event = KeyEvent { key: Key::ArrowRight, modifiers: Modifiers::empty() };
        assert_eq!(encode(event), b"\x1b[C");
    }

    #[test]
    fn arrow_left() {
        let event = KeyEvent { key: Key::ArrowLeft, modifiers: Modifiers::empty() };
        assert_eq!(encode(event), b"\x1b[D");
    }

    // --- Navigation keys ---

    #[test]
    fn home() {
        let event = KeyEvent { key: Key::Home, modifiers: Modifiers::empty() };
        assert_eq!(encode(event), b"\x1b[H");
    }

    #[test]
    fn end() {
        let event = KeyEvent { key: Key::End, modifiers: Modifiers::empty() };
        assert_eq!(encode(event), b"\x1b[F");
    }

    #[test]
    fn page_up() {
        let event = KeyEvent { key: Key::PageUp, modifiers: Modifiers::empty() };
        assert_eq!(encode(event), b"\x1b[5~");
    }

    #[test]
    fn page_down() {
        let event = KeyEvent { key: Key::PageDown, modifiers: Modifiers::empty() };
        assert_eq!(encode(event), b"\x1b[6~");
    }

    // --- Ctrl + character ---

    #[test]
    fn ctrl_a() {
        let event = KeyEvent { key: Key::Char('a'), modifiers: Modifiers::CTRL };
        assert_eq!(encode(event), b"\x01");
    }

    #[test]
    fn ctrl_c() {
        let event = KeyEvent { key: Key::Char('c'), modifiers: Modifiers::CTRL };
        assert_eq!(encode(event), b"\x03");
    }

    #[test]
    fn ctrl_z() {
        let event = KeyEvent { key: Key::Char('z'), modifiers: Modifiers::CTRL };
        assert_eq!(encode(event), b"\x1a");
    }

    #[test]
    fn ctrl_uppercase_a() {
        // Ctrl+A and Ctrl+Shift+A should both produce 0x01
        let event = KeyEvent { key: Key::Char('A'), modifiers: Modifiers::CTRL };
        assert_eq!(encode(event), b"\x01");
    }

    // --- Alt + character ---

    #[test]
    fn alt_a() {
        let event = KeyEvent { key: Key::Char('a'), modifiers: Modifiers::ALT };
        assert_eq!(encode(event), b"\x1ba");
    }

    #[test]
    fn alt_uppercase() {
        let event = KeyEvent { key: Key::Char('A'), modifiers: Modifiers::ALT };
        assert_eq!(encode(event), b"\x1bA");
    }

    // --- Alt + special keys ---

    #[test]
    fn alt_arrow_up() {
        let event = KeyEvent { key: Key::ArrowUp, modifiers: Modifiers::ALT };
        assert_eq!(encode(event), b"\x1b[1;3A");
    }

    #[test]
    fn alt_arrow_down() {
        let event = KeyEvent { key: Key::ArrowDown, modifiers: Modifiers::ALT };
        assert_eq!(encode(event), b"\x1b[1;3B");
    }

    // --- Shift + arrow (modified cursor keys) ---

    #[test]
    fn shift_arrow_up() {
        let event = KeyEvent { key: Key::ArrowUp, modifiers: Modifiers::SHIFT };
        assert_eq!(encode(event), b"\x1b[1;2A");
    }

    #[test]
    fn shift_arrow_right() {
        let event = KeyEvent { key: Key::ArrowRight, modifiers: Modifiers::SHIFT };
        assert_eq!(encode(event), b"\x1b[1;2C");
    }

    // --- Ctrl + arrow ---

    #[test]
    fn ctrl_arrow_left() {
        let event = KeyEvent { key: Key::ArrowLeft, modifiers: Modifiers::CTRL };
        assert_eq!(encode(event), b"\x1b[1;5D");
    }

    #[test]
    fn ctrl_arrow_right() {
        let event = KeyEvent { key: Key::ArrowRight, modifiers: Modifiers::CTRL };
        assert_eq!(encode(event), b"\x1b[1;5C");
    }

    // --- Ctrl+Alt combination ---

    #[test]
    fn ctrl_alt_arrow_up() {
        let event = KeyEvent { key: Key::ArrowUp, modifiers: Modifiers::CTRL | Modifiers::ALT };
        assert_eq!(encode(event), b"\x1b[1;7A");
    }

    // --- Shift char (no special encoding) ---

    #[test]
    fn shift_char_is_just_the_char() {
        // Shift is already reflected in the char value (e.g. 'A' instead of 'a')
        let event = KeyEvent { key: Key::Char('A'), modifiers: Modifiers::SHIFT };
        assert_eq!(encode(event), b"A");
    }

    // --- Alt + backspace ---

    #[test]
    fn alt_backspace() {
        let event = KeyEvent { key: Key::Backspace, modifiers: Modifiers::ALT };
        assert_eq!(encode(event), b"\x1b\x7f");
    }

    // --- Edge: Ctrl + non-alpha ---

    #[test]
    fn ctrl_non_alpha_ignored() {
        // Ctrl+1 has no standard encoding → send '1' as-is
        let event = KeyEvent { key: Key::Char('1'), modifiers: Modifiers::CTRL };
        assert_eq!(encode(event), b"1");
    }

    #[test]
    fn kitty_disambiguate_encodes_ctrl_c_as_csi_u() {
        let event = KeyEvent { key: Key::Char('c'), modifiers: Modifiers::CTRL };
        assert_eq!(
            encode_with_kitty_flags(event, KITTY_KEYBOARD_DISAMBIGUATE_ESCAPES),
            b"\x1b[99;5u"
        );
    }

    #[test]
    fn kitty_disambiguate_encodes_alt_shift_text_key_as_csi_u() {
        let event = KeyEvent { key: Key::Char('A'), modifiers: Modifiers::ALT | Modifiers::SHIFT };
        assert_eq!(
            encode_with_kitty_flags(event, KITTY_KEYBOARD_DISAMBIGUATE_ESCAPES),
            b"\x1b[97;4u"
        );
    }

    #[test]
    fn kitty_disambiguate_keeps_plain_text_as_utf8() {
        let event = KeyEvent { key: Key::Char('a'), modifiers: Modifiers::empty() };
        assert_eq!(
            encode_with_kitty_flags(event, KITTY_KEYBOARD_DISAMBIGUATE_ESCAPES),
            b"a"
        );
    }

    #[test]
    fn kitty_report_all_encodes_plain_text_as_csi_u() {
        let event = KeyEvent { key: Key::Char('a'), modifiers: Modifiers::empty() };
        assert_eq!(
            encode_with_kitty_flags(event, KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESCAPES),
            b"\x1b[97;1u"
        );
    }

    #[test]
    fn kitty_report_all_encodes_enter_as_csi_u() {
        let event = KeyEvent { key: Key::Enter, modifiers: Modifiers::empty() };
        assert_eq!(
            encode_with_kitty_flags(event, KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESCAPES),
            b"\x1b[13;1u"
        );
    }

    #[test]
    fn kitty_report_all_uses_base_key_for_shifted_digit() {
        let event = KeyEvent { key: Key::Char('#'), modifiers: Modifiers::SHIFT };
        assert_eq!(
            encode_with_kitty_flags(event, KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESCAPES),
            b"\x1b[51;2u"
        );
    }

    #[test]
    fn kitty_repeat_arrow_up_reports_event_type() {
        let event = KeyEvent { key: Key::ArrowUp, modifiers: Modifiers::empty() };
        assert_eq!(
            encode_with_kitty_flags_and_event_type(
                event,
                KITTY_KEYBOARD_REPORT_EVENT_TYPES,
                KeyEventType::Repeat,
            ),
            b"\x1b[1;1:2A"
        );
    }

    #[test]
    fn kitty_release_f1_reports_event_type() {
        let event = KeyEvent { key: Key::F1, modifiers: Modifiers::empty() };
        assert_eq!(
            encode_with_kitty_flags_and_event_type(
                event,
                KITTY_KEYBOARD_REPORT_EVENT_TYPES,
                KeyEventType::Release,
            ),
            b"\x1b[1;1:3P"
        );
    }

    #[test]
    fn repeat_plain_text_without_report_all_sends_utf8() {
        let event = KeyEvent { key: Key::Char('a'), modifiers: Modifiers::empty() };
        assert_eq!(
            encode_with_kitty_flags_and_event_type(
                event,
                KITTY_KEYBOARD_REPORT_EVENT_TYPES,
                KeyEventType::Repeat,
            ),
            b"a"
        );
    }
}
