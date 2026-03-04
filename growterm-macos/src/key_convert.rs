use crate::event::Modifiers;

/// macOS 가상 키코드 (Carbon kVK_ 상수)
pub mod keycode {
    pub const RETURN: u16 = 0x24;
    pub const TAB: u16 = 0x30;
    pub const SPACE: u16 = 0x31;
    pub const DELETE: u16 = 0x33; // Backspace
    pub const ESCAPE: u16 = 0x35;
    pub const FORWARD_DELETE: u16 = 0x75;
    pub const UP_ARROW: u16 = 0x7E;
    pub const DOWN_ARROW: u16 = 0x7D;
    pub const LEFT_ARROW: u16 = 0x7B;
    pub const RIGHT_ARROW: u16 = 0x7C;
    pub const HOME: u16 = 0x73;
    pub const END: u16 = 0x77;
    pub const PAGE_UP: u16 = 0x74;
    pub const PAGE_DOWN: u16 = 0x79;
    pub const ANSI_A: u16 = 0x00;
    pub const ANSI_C: u16 = 0x08;
    pub const ANSI_Q: u16 = 0x0C;
    pub const ANSI_V: u16 = 0x09;
    pub const ANSI_EQUAL: u16 = 0x18;
    pub const ANSI_MINUS: u16 = 0x1B;
    pub const ANSI_T: u16 = 0x11;
    pub const ANSI_W: u16 = 0x0D;
    pub const ANSI_P: u16 = 0x23;
    pub const ANSI_LEFT_BRACKET: u16 = 0x21;
    pub const ANSI_RIGHT_BRACKET: u16 = 0x1E;
    pub const ANSI_1: u16 = 0x12;
    pub const ANSI_2: u16 = 0x13;
    pub const ANSI_3: u16 = 0x14;
    pub const ANSI_4: u16 = 0x15;
    pub const ANSI_5: u16 = 0x17;
    pub const ANSI_6: u16 = 0x16;
    pub const ANSI_7: u16 = 0x1A;
    pub const ANSI_8: u16 = 0x1C;
    pub const ANSI_9: u16 = 0x19;
    pub const ANSI_H: u16 = 0x04;
    pub const ANSI_J: u16 = 0x26;
    pub const ANSI_K: u16 = 0x28;
    pub const ANSI_L: u16 = 0x25;
    pub const ANSI_D: u16 = 0x02;
    pub const ANSI_U: u16 = 0x20;
    pub const ANSI_N: u16 = 0x2D;
    pub const ANSI_O: u16 = 0x1F;
    pub const ANSI_Y: u16 = 0x10;
    pub const ANSI_R: u16 = 0x0F;
    pub const ANSI_GRAVE: u16 = 0x32; // ` (backtick / ₩)
}

/// 문자열 → macOS keycode 변환 (복사모드 키 설정용)
pub fn char_to_keycode(s: &str) -> Option<u16> {
    match s {
        "a" => Some(keycode::ANSI_A),
        "c" => Some(keycode::ANSI_C),
        "d" => Some(keycode::ANSI_D),
        "h" => Some(keycode::ANSI_H),
        "j" => Some(keycode::ANSI_J),
        "k" => Some(keycode::ANSI_K),
        "l" => Some(keycode::ANSI_L),
        "n" => Some(keycode::ANSI_N),
        "o" => Some(keycode::ANSI_O),
        "p" => Some(keycode::ANSI_P),
        "q" => Some(keycode::ANSI_Q),
        "r" => Some(keycode::ANSI_R),
        "t" => Some(keycode::ANSI_T),
        "u" => Some(keycode::ANSI_U),
        "v" => Some(keycode::ANSI_V),
        "w" => Some(keycode::ANSI_W),
        "y" => Some(keycode::ANSI_Y),
        "Escape" => Some(keycode::ESCAPE),
        "`" => Some(keycode::ANSI_GRAVE),
        _ => None,
    }
}

/// macOS keycode + characters → growterm_types::KeyEvent 변환
pub fn convert_key(
    keycode: u16,
    characters: Option<&str>,
    modifiers: Modifiers,
) -> Option<growterm_types::KeyEvent> {
    let mods = convert_modifiers(modifiers);

    let key = match keycode {
        keycode::RETURN => growterm_types::Key::Enter,
        keycode::TAB => growterm_types::Key::Tab,
        keycode::ESCAPE => growterm_types::Key::Escape,
        keycode::DELETE => growterm_types::Key::Backspace,
        keycode::FORWARD_DELETE => growterm_types::Key::Delete,
        keycode::UP_ARROW => growterm_types::Key::ArrowUp,
        keycode::DOWN_ARROW => growterm_types::Key::ArrowDown,
        keycode::LEFT_ARROW => growterm_types::Key::ArrowLeft,
        keycode::RIGHT_ARROW => growterm_types::Key::ArrowRight,
        keycode::HOME => growterm_types::Key::Home,
        keycode::END => growterm_types::Key::End,
        keycode::PAGE_UP => growterm_types::Key::PageUp,
        keycode::PAGE_DOWN => growterm_types::Key::PageDown,
        keycode::SPACE => growterm_types::Key::Char(' '),
        _ => {
            // 문자 키: characters에서 추출
            let c = characters.and_then(|s| {
                let mut chars = s.chars();
                let c = chars.next()?;
                if chars.next().is_some() {
                    return None;
                }
                Some(c)
            })?;
            growterm_types::Key::Char(c)
        }
    };

    Some(growterm_types::KeyEvent {
        key,
        modifiers: mods,
    })
}

fn convert_modifiers(modifiers: Modifiers) -> growterm_types::Modifiers {
    let mut mods = growterm_types::Modifiers::empty();
    if modifiers.contains(Modifiers::CONTROL) {
        mods |= growterm_types::Modifiers::CTRL;
    }
    if modifiers.contains(Modifiers::ALT) {
        mods |= growterm_types::Modifiers::ALT;
    }
    if modifiers.contains(Modifiers::SHIFT) {
        mods |= growterm_types::Modifiers::SHIFT;
    }
    mods
}

#[cfg(test)]
mod tests {
    use super::*;
    use growterm_types::{Key, KeyEvent, Modifiers as TypeMods};

    #[test]
    fn return_key() {
        let result = convert_key(keycode::RETURN, None, Modifiers::empty());
        assert_eq!(
            result,
            Some(KeyEvent { key: Key::Enter, modifiers: TypeMods::empty() })
        );
    }

    #[test]
    fn tab_key() {
        let result = convert_key(keycode::TAB, None, Modifiers::empty());
        assert_eq!(
            result,
            Some(KeyEvent { key: Key::Tab, modifiers: TypeMods::empty() })
        );
    }

    #[test]
    fn escape_key() {
        let result = convert_key(keycode::ESCAPE, None, Modifiers::empty());
        assert_eq!(
            result,
            Some(KeyEvent { key: Key::Escape, modifiers: TypeMods::empty() })
        );
    }

    #[test]
    fn backspace_key() {
        let result = convert_key(keycode::DELETE, None, Modifiers::empty());
        assert_eq!(
            result,
            Some(KeyEvent { key: Key::Backspace, modifiers: TypeMods::empty() })
        );
    }

    #[test]
    fn forward_delete_key() {
        let result = convert_key(keycode::FORWARD_DELETE, None, Modifiers::empty());
        assert_eq!(
            result,
            Some(KeyEvent { key: Key::Delete, modifiers: TypeMods::empty() })
        );
    }

    #[test]
    fn arrow_keys() {
        assert_eq!(
            convert_key(keycode::UP_ARROW, None, Modifiers::empty()).unwrap().key,
            Key::ArrowUp
        );
        assert_eq!(
            convert_key(keycode::DOWN_ARROW, None, Modifiers::empty()).unwrap().key,
            Key::ArrowDown
        );
        assert_eq!(
            convert_key(keycode::LEFT_ARROW, None, Modifiers::empty()).unwrap().key,
            Key::ArrowLeft
        );
        assert_eq!(
            convert_key(keycode::RIGHT_ARROW, None, Modifiers::empty()).unwrap().key,
            Key::ArrowRight
        );
    }

    #[test]
    fn home_end_page_keys() {
        assert_eq!(
            convert_key(keycode::HOME, None, Modifiers::empty()).unwrap().key,
            Key::Home
        );
        assert_eq!(
            convert_key(keycode::END, None, Modifiers::empty()).unwrap().key,
            Key::End
        );
        assert_eq!(
            convert_key(keycode::PAGE_UP, None, Modifiers::empty()).unwrap().key,
            Key::PageUp
        );
        assert_eq!(
            convert_key(keycode::PAGE_DOWN, None, Modifiers::empty()).unwrap().key,
            Key::PageDown
        );
    }

    #[test]
    fn space_key() {
        let result = convert_key(keycode::SPACE, None, Modifiers::empty());
        assert_eq!(
            result,
            Some(KeyEvent { key: Key::Char(' '), modifiers: TypeMods::empty() })
        );
    }

    #[test]
    fn character_key() {
        let result = convert_key(0x00, Some("a"), Modifiers::empty()); // 0x00 = kVK_ANSI_A
        assert_eq!(
            result,
            Some(KeyEvent { key: Key::Char('a'), modifiers: TypeMods::empty() })
        );
    }

    #[test]
    fn ctrl_c() {
        let result = convert_key(0x08, Some("c"), Modifiers::CONTROL); // 0x08 = kVK_ANSI_C
        assert_eq!(
            result,
            Some(KeyEvent { key: Key::Char('c'), modifiers: TypeMods::CTRL })
        );
    }

    #[test]
    fn alt_d() {
        let result = convert_key(0x02, Some("d"), Modifiers::ALT); // 0x02 = kVK_ANSI_D
        assert_eq!(
            result,
            Some(KeyEvent { key: Key::Char('d'), modifiers: TypeMods::ALT })
        );
    }

    #[test]
    fn shift_arrow() {
        let result = convert_key(keycode::UP_ARROW, None, Modifiers::SHIFT | Modifiers::CONTROL);
        let result = result.unwrap();
        assert_eq!(result.key, Key::ArrowUp);
        assert!(result.modifiers.contains(TypeMods::CTRL));
        assert!(result.modifiers.contains(TypeMods::SHIFT));
    }

    #[test]
    fn unknown_keycode_no_characters_returns_none() {
        let result = convert_key(0xFF, None, Modifiers::empty());
        assert_eq!(result, None);
    }

    #[test]
    fn multi_char_characters_returns_none() {
        let result = convert_key(0x00, Some("ab"), Modifiers::empty());
        assert_eq!(result, None);
    }

    #[test]
    fn char_to_keycode_letters() {
        assert_eq!(char_to_keycode("j"), Some(keycode::ANSI_J));
        assert_eq!(char_to_keycode("k"), Some(keycode::ANSI_K));
        assert_eq!(char_to_keycode("h"), Some(keycode::ANSI_H));
        assert_eq!(char_to_keycode("l"), Some(keycode::ANSI_L));
        assert_eq!(char_to_keycode("v"), Some(keycode::ANSI_V));
        assert_eq!(char_to_keycode("y"), Some(keycode::ANSI_Y));
        assert_eq!(char_to_keycode("q"), Some(keycode::ANSI_Q));
        assert_eq!(char_to_keycode("d"), Some(keycode::ANSI_D));
        assert_eq!(char_to_keycode("u"), Some(keycode::ANSI_U));
    }

    #[test]
    fn char_to_keycode_special() {
        assert_eq!(char_to_keycode("Escape"), Some(keycode::ESCAPE));
        assert_eq!(char_to_keycode("`"), Some(keycode::ANSI_GRAVE));
    }

    #[test]
    fn char_to_keycode_unknown() {
        assert_eq!(char_to_keycode("z"), None);
        assert_eq!(char_to_keycode("F1"), None);
    }
}
