use crate::event::Modifiers;
use winit::keyboard::KeyCode;

pub mod keycode {
    pub const F1: u16 = 0x7A;
    pub const F2: u16 = 0x78;
    pub const F3: u16 = 0x63;
    pub const F4: u16 = 0x76;
    pub const F5: u16 = 0x60;
    pub const F6: u16 = 0x61;
    pub const F7: u16 = 0x62;
    pub const F8: u16 = 0x64;
    pub const F9: u16 = 0x65;
    pub const F10: u16 = 0x6D;
    pub const F11: u16 = 0x67;
    pub const F12: u16 = 0x6F;
    pub const RETURN: u16 = 0x24;
    pub const TAB: u16 = 0x30;
    pub const SPACE: u16 = 0x31;
    pub const DELETE: u16 = 0x33;
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
    pub const ANSI_F: u16 = 0x03;
    pub const ANSI_U: u16 = 0x20;
    pub const ANSI_N: u16 = 0x2D;
    pub const ANSI_O: u16 = 0x1F;
    pub const ANSI_Y: u16 = 0x10;
    pub const ANSI_R: u16 = 0x0F;
    pub const ANSI_GRAVE: u16 = 0x32;
}

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

pub fn physical_keycode_to_app_keycode(key: KeyCode) -> Option<u16> {
    match key {
        KeyCode::F1 => Some(keycode::F1),
        KeyCode::F2 => Some(keycode::F2),
        KeyCode::F3 => Some(keycode::F3),
        KeyCode::F4 => Some(keycode::F4),
        KeyCode::F5 => Some(keycode::F5),
        KeyCode::F6 => Some(keycode::F6),
        KeyCode::F7 => Some(keycode::F7),
        KeyCode::F8 => Some(keycode::F8),
        KeyCode::F9 => Some(keycode::F9),
        KeyCode::F10 => Some(keycode::F10),
        KeyCode::F11 => Some(keycode::F11),
        KeyCode::F12 => Some(keycode::F12),
        KeyCode::Enter => Some(keycode::RETURN),
        KeyCode::Tab => Some(keycode::TAB),
        KeyCode::Space => Some(keycode::SPACE),
        KeyCode::Backspace => Some(keycode::DELETE),
        KeyCode::Escape => Some(keycode::ESCAPE),
        KeyCode::Delete => Some(keycode::FORWARD_DELETE),
        KeyCode::ArrowUp => Some(keycode::UP_ARROW),
        KeyCode::ArrowDown => Some(keycode::DOWN_ARROW),
        KeyCode::ArrowLeft => Some(keycode::LEFT_ARROW),
        KeyCode::ArrowRight => Some(keycode::RIGHT_ARROW),
        KeyCode::Home => Some(keycode::HOME),
        KeyCode::End => Some(keycode::END),
        KeyCode::PageUp => Some(keycode::PAGE_UP),
        KeyCode::PageDown => Some(keycode::PAGE_DOWN),
        KeyCode::KeyA => Some(keycode::ANSI_A),
        KeyCode::KeyC => Some(keycode::ANSI_C),
        KeyCode::KeyD => Some(keycode::ANSI_D),
        KeyCode::KeyF => Some(keycode::ANSI_F),
        KeyCode::KeyH => Some(keycode::ANSI_H),
        KeyCode::KeyJ => Some(keycode::ANSI_J),
        KeyCode::KeyK => Some(keycode::ANSI_K),
        KeyCode::KeyL => Some(keycode::ANSI_L),
        KeyCode::KeyN => Some(keycode::ANSI_N),
        KeyCode::KeyO => Some(keycode::ANSI_O),
        KeyCode::KeyP => Some(keycode::ANSI_P),
        KeyCode::KeyQ => Some(keycode::ANSI_Q),
        KeyCode::KeyR => Some(keycode::ANSI_R),
        KeyCode::KeyT => Some(keycode::ANSI_T),
        KeyCode::KeyU => Some(keycode::ANSI_U),
        KeyCode::KeyV => Some(keycode::ANSI_V),
        KeyCode::KeyW => Some(keycode::ANSI_W),
        KeyCode::KeyY => Some(keycode::ANSI_Y),
        KeyCode::Digit1 => Some(keycode::ANSI_1),
        KeyCode::Digit2 => Some(keycode::ANSI_2),
        KeyCode::Digit3 => Some(keycode::ANSI_3),
        KeyCode::Digit4 => Some(keycode::ANSI_4),
        KeyCode::Digit5 => Some(keycode::ANSI_5),
        KeyCode::Digit6 => Some(keycode::ANSI_6),
        KeyCode::Digit7 => Some(keycode::ANSI_7),
        KeyCode::Digit8 => Some(keycode::ANSI_8),
        KeyCode::Digit9 => Some(keycode::ANSI_9),
        KeyCode::Equal => Some(keycode::ANSI_EQUAL),
        KeyCode::Minus => Some(keycode::ANSI_MINUS),
        KeyCode::BracketLeft => Some(keycode::ANSI_LEFT_BRACKET),
        KeyCode::BracketRight => Some(keycode::ANSI_RIGHT_BRACKET),
        KeyCode::Backquote => Some(keycode::ANSI_GRAVE),
        _ => None,
    }
}

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
        keycode::F1 => growterm_types::Key::F1,
        keycode::F2 => growterm_types::Key::F2,
        keycode::F3 => growterm_types::Key::F3,
        keycode::F4 => growterm_types::Key::F4,
        keycode::F5 => growterm_types::Key::F5,
        keycode::F6 => growterm_types::Key::F6,
        keycode::F7 => growterm_types::Key::F7,
        keycode::F8 => growterm_types::Key::F8,
        keycode::F9 => growterm_types::Key::F9,
        keycode::F10 => growterm_types::Key::F10,
        keycode::F11 => growterm_types::Key::F11,
        keycode::F12 => growterm_types::Key::F12,
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
    fn maps_winit_letters_to_existing_app_keycodes() {
        assert_eq!(
            physical_keycode_to_app_keycode(KeyCode::KeyJ),
            Some(keycode::ANSI_J)
        );
        assert_eq!(
            physical_keycode_to_app_keycode(KeyCode::KeyK),
            Some(keycode::ANSI_K)
        );
        assert_eq!(
            physical_keycode_to_app_keycode(KeyCode::KeyC),
            Some(keycode::ANSI_C)
        );
    }

    #[test]
    fn maps_winit_navigation_keys_to_existing_app_keycodes() {
        assert_eq!(
            physical_keycode_to_app_keycode(KeyCode::Enter),
            Some(keycode::RETURN)
        );
        assert_eq!(
            physical_keycode_to_app_keycode(KeyCode::ArrowUp),
            Some(keycode::UP_ARROW)
        );
        assert_eq!(
            physical_keycode_to_app_keycode(KeyCode::PageDown),
            Some(keycode::PAGE_DOWN)
        );
    }

    #[test]
    fn converts_key_with_modifiers_for_pty_encoding() {
        assert_eq!(
            convert_key(keycode::ANSI_C, Some("c"), Modifiers::CONTROL),
            Some(KeyEvent {
                key: Key::Char('c'),
                modifiers: TypeMods::CTRL,
            })
        );
    }

    #[test]
    fn char_to_keycode_supports_copy_mode_defaults() {
        assert_eq!(char_to_keycode("j"), Some(keycode::ANSI_J));
        assert_eq!(char_to_keycode("Escape"), Some(keycode::ESCAPE));
        assert_eq!(char_to_keycode("`"), Some(keycode::ANSI_GRAVE));
    }
}
