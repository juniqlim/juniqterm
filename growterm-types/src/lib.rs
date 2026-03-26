use bitflags::bitflags;

// --- Rgb ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

// --- Color ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Default,
    Indexed(u8),
    Rgb(Rgb),
}

impl Default for Color {
    fn default() -> Self {
        Color::Default
    }
}

// --- CellFlags ---

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct CellFlags: u16 {
        const BOLD          = 0b0_0000_0001;
        const DIM           = 0b0_0000_0010;
        const ITALIC        = 0b0_0000_0100;
        const UNDERLINE     = 0b0_0000_1000;
        const INVERSE       = 0b0_0001_0000;
        const HIDDEN        = 0b0_0010_0000;
        const STRIKETHROUGH = 0b0_0100_0000;
        const WIDE_CHAR     = 0b0_1000_0000;
        const OVERLINE      = 0b1_0000_0000;
    }
}

// --- Cell ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub character: char,
    pub fg: Color,
    pub bg: Color,
    pub underline_color: Color,
    pub flags: CellFlags,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            character: ' ',
            fg: Color::Default,
            bg: Color::Default,
            underline_color: Color::Default,
            flags: CellFlags::empty(),
        }
    }
}

// --- RenderCommand ---

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderCommand {
    pub col: u16,
    pub row: u16,
    pub character: char,
    pub fg: Rgb,
    pub bg: Rgb,
    pub underline_color: Option<Rgb>,
    pub flags: CellFlags,
}

// --- TerminalCommand ---

#[derive(Debug, Clone, PartialEq)]
pub enum TerminalCommand {
    Print(char),
    CursorUp(u16),
    CursorDown(u16),
    CursorForward(u16),
    CursorBack(u16),
    CursorPosition { row: u16, col: u16 },
    SetForeground(Color),
    SetBackground(Color),
    SetBold,
    SetDim,
    SetItalic,
    SetUnderline,
    SetInverse,
    SetHidden,
    SetStrikethrough,
    ResetBold,
    ResetItalic,
    ResetUnderline,
    ResetInverse,
    ResetHidden,
    ResetStrikethrough,
    SetOverline,
    ResetOverline,
    SetUnderlineColor(Color),
    ResetUnderlineColor,
    ResetAttributes,
    EraseInLine(u16),
    EraseInDisplay(u16),
    Newline,
    ReverseIndex,
    CarriageReturn,
    Backspace,
    Tab,
    Bell,
    DeleteChars(u16),
    InsertChars(u16),
    EraseChars(u16),
    InsertLines(u16),
    DeleteLines(u16),
    ScrollUp(u16),
    ScrollDown(u16),
    CursorColumn(u16),
    CursorRow(u16),
    SaveCursor,
    RestoreCursor,
    SetScrollRegion { top: u16, bottom: u16 },
    EnterAltScreen,
    LeaveAltScreen,
    ShowCursor,
    HideCursor,
}

// --- Key & Modifiers ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Tab,
    Escape,
    Backspace,
    Delete,
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
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct Modifiers: u8 {
        const CTRL  = 0b001;
        const ALT   = 0b010;
        const SHIFT = 0b100;
    }
}

// --- KeyEvent ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: Key,
    pub modifiers: Modifiers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEventType {
    Press,
    Repeat,
    Release,
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Rgb ---
    #[test]
    fn rgb_default_is_black() {
        let rgb = Rgb::default();
        assert_eq!(rgb, Rgb { r: 0, g: 0, b: 0 });
    }

    #[test]
    fn rgb_new() {
        let rgb = Rgb::new(255, 128, 0);
        assert_eq!(rgb.r, 255);
        assert_eq!(rgb.g, 128);
        assert_eq!(rgb.b, 0);
    }

    // --- Color ---
    #[test]
    fn color_default_is_named_default() {
        let color = Color::default();
        assert_eq!(color, Color::Default);
    }

    #[test]
    fn color_indexed() {
        let color = Color::Indexed(196);
        if let Color::Indexed(idx) = color {
            assert_eq!(idx, 196);
        } else {
            panic!("expected Indexed variant");
        }
    }

    #[test]
    fn color_rgb() {
        let color = Color::Rgb(Rgb::new(10, 20, 30));
        if let Color::Rgb(rgb) = color {
            assert_eq!(rgb, Rgb::new(10, 20, 30));
        } else {
            panic!("expected Rgb variant");
        }
    }

    // --- CellFlags ---
    #[test]
    fn cell_flags_default_is_empty() {
        let flags = CellFlags::default();
        assert!(flags.is_empty());
    }

    #[test]
    fn cell_flags_combine() {
        let flags = CellFlags::BOLD | CellFlags::UNDERLINE;
        assert!(flags.contains(CellFlags::BOLD));
        assert!(flags.contains(CellFlags::UNDERLINE));
        assert!(!flags.contains(CellFlags::INVERSE));
    }

    #[test]
    fn cell_flags_all_variants_distinct() {
        let all = [
            CellFlags::BOLD,
            CellFlags::DIM,
            CellFlags::ITALIC,
            CellFlags::UNDERLINE,
            CellFlags::INVERSE,
            CellFlags::HIDDEN,
            CellFlags::STRIKETHROUGH,
            CellFlags::WIDE_CHAR,
        ];
        for (i, a) in all.iter().enumerate() {
            for (j, b) in all.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b);
                    assert!(!a.intersects(*b));
                }
            }
        }
    }

    // --- Cell ---
    #[test]
    fn cell_default() {
        let cell = Cell::default();
        assert_eq!(cell.character, ' ');
        assert_eq!(cell.fg, Color::Default);
        assert_eq!(cell.bg, Color::Default);
        assert!(cell.flags.is_empty());
    }

    // --- RenderCommand ---
    #[test]
    fn render_command_fields() {
        let cmd = RenderCommand {
            col: 5,
            row: 10,
            character: 'A',
            fg: Rgb::new(255, 255, 255),
            bg: Rgb::new(0, 0, 0),
            underline_color: None,
            flags: CellFlags::BOLD,
        };
        assert_eq!(cmd.col, 5);
        assert_eq!(cmd.row, 10);
        assert_eq!(cmd.character, 'A');
    }

    // --- TerminalCommand ---
    #[test]
    fn terminal_command_print() {
        let cmd = TerminalCommand::Print('X');
        if let TerminalCommand::Print(c) = cmd {
            assert_eq!(c, 'X');
        } else {
            panic!("expected Print variant");
        }
    }

    #[test]
    fn terminal_command_cursor_movements() {
        assert!(matches!(TerminalCommand::CursorUp(1), TerminalCommand::CursorUp(1)));
        assert!(matches!(TerminalCommand::CursorDown(2), TerminalCommand::CursorDown(2)));
        assert!(matches!(TerminalCommand::CursorForward(3), TerminalCommand::CursorForward(3)));
        assert!(matches!(TerminalCommand::CursorBack(4), TerminalCommand::CursorBack(4)));
        assert!(matches!(TerminalCommand::CursorPosition { row: 1, col: 2 }, TerminalCommand::CursorPosition { row: 1, col: 2 }));
    }

    #[test]
    fn terminal_command_sgr() {
        let cmd = TerminalCommand::SetForeground(Color::Rgb(Rgb::new(255, 0, 0)));
        if let TerminalCommand::SetForeground(Color::Rgb(rgb)) = cmd {
            assert_eq!(rgb, Rgb::new(255, 0, 0));
        } else {
            panic!("expected SetForeground with Rgb");
        }
    }

    #[test]
    fn terminal_command_erase_and_newline() {
        assert!(matches!(TerminalCommand::EraseInLine(0), TerminalCommand::EraseInLine(0)));
        assert!(matches!(TerminalCommand::EraseInDisplay(2), TerminalCommand::EraseInDisplay(2)));
        assert!(matches!(TerminalCommand::Newline, TerminalCommand::Newline));
        assert!(matches!(TerminalCommand::CarriageReturn, TerminalCommand::CarriageReturn));
        assert!(matches!(TerminalCommand::Backspace, TerminalCommand::Backspace));
    }

    // --- KeyEvent ---
    #[test]
    fn key_event_fields() {
        let event = KeyEvent {
            key: Key::Char('a'),
            modifiers: Modifiers::CTRL,
        };
        assert_eq!(event.key, Key::Char('a'));
        assert!(event.modifiers.contains(Modifiers::CTRL));
    }

    #[test]
    fn key_event_type_variants_distinct() {
        assert_ne!(KeyEventType::Press, KeyEventType::Repeat);
        assert_ne!(KeyEventType::Repeat, KeyEventType::Release);
    }

    #[test]
    fn modifiers_combine() {
        let mods = Modifiers::CTRL | Modifiers::SHIFT;
        assert!(mods.contains(Modifiers::CTRL));
        assert!(mods.contains(Modifiers::SHIFT));
        assert!(!mods.contains(Modifiers::ALT));
    }

    #[test]
    fn key_special_keys() {
        assert!(matches!(Key::Enter, Key::Enter));
        assert!(matches!(Key::Tab, Key::Tab));
        assert!(matches!(Key::Escape, Key::Escape));
        assert!(matches!(Key::ArrowUp, Key::ArrowUp));
        assert!(matches!(Key::ArrowDown, Key::ArrowDown));
        assert!(matches!(Key::ArrowLeft, Key::ArrowLeft));
        assert!(matches!(Key::ArrowRight, Key::ArrowRight));
        assert!(matches!(Key::Backspace, Key::Backspace));
        assert!(matches!(Key::Delete, Key::Delete));
        assert!(matches!(Key::Home, Key::Home));
        assert!(matches!(Key::End, Key::End));
        assert!(matches!(Key::PageUp, Key::PageUp));
        assert!(matches!(Key::PageDown, Key::PageDown));
    }
}
