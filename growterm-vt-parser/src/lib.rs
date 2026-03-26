use growterm_types::{Color, Rgb, TerminalCommand};

struct Handler {
    commands: Vec<TerminalCommand>,
}

impl Handler {
    fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    fn take(&mut self) -> Vec<TerminalCommand> {
        std::mem::take(&mut self.commands)
    }

    fn handle_sgr(&mut self, params: &vte::Params) {
        let parts: Vec<&[u16]> = params.iter().collect();
        let mut i = 0usize;
        while i < parts.len() {
            let part = parts[i];
            let param = part.first().copied().unwrap_or(0);
            match param {
                0 => self.commands.push(TerminalCommand::ResetAttributes),
                1 => self.commands.push(TerminalCommand::SetBold),
                2 => self.commands.push(TerminalCommand::SetDim),
                3 => self.commands.push(TerminalCommand::SetItalic),
                4 => {
                    // SGR 4:0 means reset underline (sub-parameter form)
                    if part.len() > 1 && part[1] == 0 {
                        self.commands.push(TerminalCommand::ResetUnderline);
                    } else {
                        self.commands.push(TerminalCommand::SetUnderline);
                    }
                }
                7 => self.commands.push(TerminalCommand::SetInverse),
                8 => self.commands.push(TerminalCommand::SetHidden),
                9 => self.commands.push(TerminalCommand::SetStrikethrough),
                22 => self.commands.push(TerminalCommand::ResetBold),
                23 => self.commands.push(TerminalCommand::ResetItalic),
                24 => self.commands.push(TerminalCommand::ResetUnderline),
                27 => self.commands.push(TerminalCommand::ResetInverse),
                28 => self.commands.push(TerminalCommand::ResetHidden),
                29 => self.commands.push(TerminalCommand::ResetStrikethrough),
                53 => self.commands.push(TerminalCommand::SetOverline),
                55 => self.commands.push(TerminalCommand::ResetOverline),
                // Standard foreground colors 30-37
                30..=37 => {
                    self.commands
                        .push(TerminalCommand::SetForeground(Color::Indexed(
                            (param - 30) as u8,
                        )));
                }
                38 => {
                    if let Some((color, consumed)) = self.parse_extended_color(&parts, i) {
                        self.commands.push(TerminalCommand::SetForeground(color));
                        i += consumed;
                    }
                }
                39 => self
                    .commands
                    .push(TerminalCommand::SetForeground(Color::Default)),
                // Standard background colors 40-47
                40..=47 => {
                    self.commands
                        .push(TerminalCommand::SetBackground(Color::Indexed(
                            (param - 40) as u8,
                        )));
                }
                48 => {
                    if let Some((color, consumed)) = self.parse_extended_color(&parts, i) {
                        self.commands.push(TerminalCommand::SetBackground(color));
                        i += consumed;
                    }
                }
                49 => self
                    .commands
                    .push(TerminalCommand::SetBackground(Color::Default)),
                58 => {
                    if let Some((color, consumed)) = self.parse_extended_color(&parts, i) {
                        self.commands
                            .push(TerminalCommand::SetUnderlineColor(color));
                        i += consumed;
                    }
                }
                59 => self
                    .commands
                    .push(TerminalCommand::ResetUnderlineColor),
                // Bright foreground colors 90-97
                90..=97 => {
                    self.commands
                        .push(TerminalCommand::SetForeground(Color::Indexed(
                            (param - 90 + 8) as u8,
                        )));
                }
                // Bright background colors 100-107
                100..=107 => {
                    self.commands
                        .push(TerminalCommand::SetBackground(Color::Indexed(
                            (param - 100 + 8) as u8,
                        )));
                }
                _ => {} // ignore unknown SGR
            }
            i += 1;
        }
    }

    fn parse_extended_color(&self, parts: &[&[u16]], i: usize) -> Option<(Color, usize)> {
        let cur = parts.get(i)?;

        // Colon form (e.g. 38:5:196 / 48:2::10:20:30) arrives as a single part.
        if cur.len() >= 2 {
            match cur[1] {
                5 => {
                    let idx = *cur.get(2)? as u8;
                    return Some((Color::Indexed(idx), 0));
                }
                2 => {
                    let rgb = Self::parse_rgb_tail(&cur[2..])?;
                    return Some((Color::Rgb(rgb), 0));
                }
                _ => return None,
            }
        }

        // Semicolon form (e.g. 38;5;196 / 48;2;10;20;30)
        let mode = *parts.get(i + 1)?.first()?;
        match mode {
            5 => {
                let idx = *parts.get(i + 2)?.first()? as u8;
                Some((Color::Indexed(idx), 2))
            }
            2 => {
                let c0 = *parts.get(i + 2)?.first()?;
                let c1 = *parts.get(i + 3)?.first()?;
                let c2 = *parts.get(i + 4)?.first()?;
                // Semicolon form is parsed as canonical RGB triplet (R;G;B).
                // Colorspace-prefixed form (0;R;G;B) is supported in colon form.
                Some((Color::Rgb(Rgb::new(c0 as u8, c1 as u8, c2 as u8)), 4))
            }
            _ => None,
        }
    }

    fn parse_rgb_tail(tail: &[u16]) -> Option<Rgb> {
        // Accept both [R,G,B] and [0,R,G,B] (colorspace id omitted/present).
        let rgb = if tail.len() >= 4 && tail[0] == 0 {
            &tail[1..4]
        } else if tail.len() >= 3 {
            &tail[..3]
        } else {
            return None;
        };
        Some(Rgb::new(rgb[0] as u8, rgb[1] as u8, rgb[2] as u8))
    }
}

impl vte::Perform for Handler {
    fn print(&mut self, c: char) {
        self.commands.push(TerminalCommand::Print(c));
    }

    fn execute(&mut self, byte: u8) {
        let cmd = match byte {
            0x07 => TerminalCommand::Bell,
            0x08 => TerminalCommand::Backspace,
            0x09 => TerminalCommand::Tab,
            0x0A => TerminalCommand::Newline,
            0x0D => TerminalCommand::CarriageReturn,
            _ => return,
        };
        self.commands.push(cmd);
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        if !intermediates.is_empty() {
            return;
        }
        match byte {
            b'M' => self.commands.push(TerminalCommand::ReverseIndex),
            b'7' => self.commands.push(TerminalCommand::SaveCursor),
            b'8' => self.commands.push(TerminalCommand::RestoreCursor),
            _ => {}
        }
    }

    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        let first = params.iter().next().map(|p| p[0]).unwrap_or(0);

        // Private mode sequences (CSI ? ... h/l)
        if intermediates == [b'?'] {
            match (action, first) {
                ('h', 25) => self.commands.push(TerminalCommand::ShowCursor),
                ('l', 25) => self.commands.push(TerminalCommand::HideCursor),
                ('h', 1049) => self.commands.push(TerminalCommand::EnterAltScreen),
                ('l', 1049) => self.commands.push(TerminalCommand::LeaveAltScreen),
                _ => {}
            }
            return;
        }

        // Ignore other private/intermediate sequences (CSI > ..., CSI = ..., etc.)
        if !intermediates.is_empty() {
            return;
        }

        match action {
            'A' => self.commands.push(TerminalCommand::CursorUp(first.max(1))),
            'B' => self
                .commands
                .push(TerminalCommand::CursorDown(first.max(1))),
            'C' => self
                .commands
                .push(TerminalCommand::CursorForward(first.max(1))),
            'D' => self
                .commands
                .push(TerminalCommand::CursorBack(first.max(1))),
            'E' => {
                self.commands
                    .push(TerminalCommand::CursorDown(first.max(1)));
                self.commands.push(TerminalCommand::CarriageReturn);
            }
            'F' => {
                self.commands.push(TerminalCommand::CursorUp(first.max(1)));
                self.commands.push(TerminalCommand::CarriageReturn);
            }
            'H' | 'f' => {
                let mut p = params.iter();
                let row = p.next().map(|v| v[0]).unwrap_or(0).max(1);
                let col = p.next().map(|v| v[0]).unwrap_or(0).max(1);
                self.commands
                    .push(TerminalCommand::CursorPosition { row, col });
            }
            'J' => self.commands.push(TerminalCommand::EraseInDisplay(first)),
            'K' => self.commands.push(TerminalCommand::EraseInLine(first)),
            'P' => self
                .commands
                .push(TerminalCommand::DeleteChars(first.max(1))),
            '@' => self
                .commands
                .push(TerminalCommand::InsertChars(first.max(1))),
            'X' => self
                .commands
                .push(TerminalCommand::EraseChars(first.max(1))),
            'L' => self
                .commands
                .push(TerminalCommand::InsertLines(first.max(1))),
            'M' => self
                .commands
                .push(TerminalCommand::DeleteLines(first.max(1))),
            'S' => self
                .commands
                .push(TerminalCommand::ScrollUp(first.max(1))),
            'T' => self
                .commands
                .push(TerminalCommand::ScrollDown(first.max(1))),
            'G' => self
                .commands
                .push(TerminalCommand::CursorColumn(first.max(1))),
            'd' => self
                .commands
                .push(TerminalCommand::CursorRow(first.max(1))),
            's' => self.commands.push(TerminalCommand::SaveCursor),
            'u' => self.commands.push(TerminalCommand::RestoreCursor),
            'r' => {
                let mut p = params.iter();
                let top = p.next().map(|v| v[0]).unwrap_or(0);
                let bottom = p.next().map(|v| v[0]).unwrap_or(0);
                self.commands
                    .push(TerminalCommand::SetScrollRegion { top, bottom });
            }
            'm' => self.handle_sgr(params),
            _ => {} // ignore unknown CSI
        }
    }
}

pub struct VtParser {
    parser: vte::Parser,
    handler: Handler,
}

impl VtParser {
    pub fn new() -> Self {
        Self {
            parser: vte::Parser::new(),
            handler: Handler::new(),
        }
    }

    pub fn parse(&mut self, bytes: &[u8]) -> Vec<TerminalCommand> {
        for &byte in bytes {
            self.parser.advance(&mut self.handler, byte);
        }
        self.handler.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- ASCII text ---

    #[test]
    fn parse_ascii_text() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"Hello");
        assert_eq!(
            cmds,
            vec![
                TerminalCommand::Print('H'),
                TerminalCommand::Print('e'),
                TerminalCommand::Print('l'),
                TerminalCommand::Print('l'),
                TerminalCommand::Print('o'),
            ]
        );
    }

    #[test]
    fn parse_empty_input() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"");
        assert!(cmds.is_empty());
    }

    // --- C0 control characters ---

    #[test]
    fn parse_newline() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\n");
        assert_eq!(cmds, vec![TerminalCommand::Newline]);
    }

    #[test]
    fn parse_carriage_return() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\r");
        assert_eq!(cmds, vec![TerminalCommand::CarriageReturn]);
    }

    #[test]
    fn parse_backspace() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x08");
        assert_eq!(cmds, vec![TerminalCommand::Backspace]);
    }

    #[test]
    fn parse_tab() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\t");
        assert_eq!(cmds, vec![TerminalCommand::Tab]);
    }

    #[test]
    fn parse_bell() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x07");
        assert_eq!(cmds, vec![TerminalCommand::Bell]);
    }

    // --- CSI cursor movement ---

    #[test]
    fn parse_cursor_up() {
        let mut parser = VtParser::new();
        // ESC [ 3 A
        let cmds = parser.parse(b"\x1b[3A");
        assert_eq!(cmds, vec![TerminalCommand::CursorUp(3)]);
    }

    #[test]
    fn parse_cursor_up_default() {
        let mut parser = VtParser::new();
        // ESC [ A (no param = default 1)
        let cmds = parser.parse(b"\x1b[A");
        assert_eq!(cmds, vec![TerminalCommand::CursorUp(1)]);
    }

    #[test]
    fn parse_cursor_down() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[5B");
        assert_eq!(cmds, vec![TerminalCommand::CursorDown(5)]);
    }

    #[test]
    fn parse_cursor_forward() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[2C");
        assert_eq!(cmds, vec![TerminalCommand::CursorForward(2)]);
    }

    #[test]
    fn parse_cursor_back() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[4D");
        assert_eq!(cmds, vec![TerminalCommand::CursorBack(4)]);
    }

    #[test]
    fn parse_cursor_next_line() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[2E");
        assert_eq!(
            cmds,
            vec![TerminalCommand::CursorDown(2), TerminalCommand::CarriageReturn]
        );
    }

    #[test]
    fn parse_cursor_previous_line() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[3F");
        assert_eq!(
            cmds,
            vec![TerminalCommand::CursorUp(3), TerminalCommand::CarriageReturn]
        );
    }

    #[test]
    fn parse_csi_save_cursor() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[s");
        assert_eq!(cmds, vec![TerminalCommand::SaveCursor]);
    }

    #[test]
    fn parse_csi_restore_cursor() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[u");
        assert_eq!(cmds, vec![TerminalCommand::RestoreCursor]);
    }

    #[test]
    fn parse_dec_save_restore_cursor() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b7\x1b8");
        assert_eq!(
            cmds,
            vec![TerminalCommand::SaveCursor, TerminalCommand::RestoreCursor]
        );
    }

    #[test]
    fn parse_reverse_index() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1bM");
        assert_eq!(cmds, vec![TerminalCommand::ReverseIndex]);
    }

    #[test]
    fn parse_cursor_position() {
        let mut parser = VtParser::new();
        // ESC [ 10 ; 20 H
        let cmds = parser.parse(b"\x1b[10;20H");
        assert_eq!(
            cmds,
            vec![TerminalCommand::CursorPosition { row: 10, col: 20 }]
        );
    }

    #[test]
    fn parse_cursor_position_default() {
        let mut parser = VtParser::new();
        // ESC [ H (no params = 1;1)
        let cmds = parser.parse(b"\x1b[H");
        assert_eq!(
            cmds,
            vec![TerminalCommand::CursorPosition { row: 1, col: 1 }]
        );
    }

    // --- SGR (Set Graphics Rendition) ---

    #[test]
    fn parse_sgr_reset() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[0m");
        assert_eq!(cmds, vec![TerminalCommand::ResetAttributes]);
    }

    #[test]
    fn parse_sgr_reset_no_param() {
        let mut parser = VtParser::new();
        // ESC [ m (no param = reset)
        let cmds = parser.parse(b"\x1b[m");
        assert_eq!(cmds, vec![TerminalCommand::ResetAttributes]);
    }

    #[test]
    fn parse_sgr_bold() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[1m");
        assert_eq!(cmds, vec![TerminalCommand::SetBold]);
    }

    #[test]
    fn parse_sgr_dim() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[2m");
        assert_eq!(cmds, vec![TerminalCommand::SetDim]);
    }

    #[test]
    fn parse_sgr_italic() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[3m");
        assert_eq!(cmds, vec![TerminalCommand::SetItalic]);
    }

    #[test]
    fn parse_sgr_underline() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[4m");
        assert_eq!(cmds, vec![TerminalCommand::SetUnderline]);
    }

    #[test]
    fn parse_sgr_underline_sub_param_zero_resets() {
        let mut parser = VtParser::new();
        // SGR 4:0 means "no underline" (sub-parameter form)
        let cmds = parser.parse(b"\x1b[4:0m");
        assert_eq!(cmds, vec![TerminalCommand::ResetUnderline]);
    }

    #[test]
    fn parse_sgr_underline_sub_param_one_sets() {
        let mut parser = VtParser::new();
        // SGR 4:1 means "single underline"
        let cmds = parser.parse(b"\x1b[4:1m");
        assert_eq!(cmds, vec![TerminalCommand::SetUnderline]);
    }

    #[test]
    fn parse_sgr_underline_sub_param_curly() {
        let mut parser = VtParser::new();
        // SGR 4:3 (curly underline) — parsed as SetUnderline
        let cmds = parser.parse(b"\x1b[4:3m");
        assert_eq!(cmds, vec![TerminalCommand::SetUnderline]);
    }

    #[test]
    fn parse_sgr_underline_sub_param_dotted() {
        let mut parser = VtParser::new();
        // SGR 4:4 (dotted) and 4:5 (dashed) — parsed as SetUnderline
        let cmds = parser.parse(b"\x1b[4:4m");
        assert_eq!(cmds, vec![TerminalCommand::SetUnderline]);
    }

    #[test]
    fn parse_sgr_overline() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[53m");
        assert_eq!(cmds, vec![TerminalCommand::SetOverline]);
    }

    #[test]
    fn parse_sgr_reset_overline() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[55m");
        assert_eq!(cmds, vec![TerminalCommand::ResetOverline]);
    }

    #[test]
    fn parse_sgr_underline_color_rgb() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[58:2::255:0:128m");
        assert_eq!(
            cmds,
            vec![TerminalCommand::SetUnderlineColor(Color::Rgb(
                Rgb::new(255, 0, 128)
            ))]
        );
    }

    #[test]
    fn parse_sgr_underline_color_256() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[58:5:196m");
        assert_eq!(
            cmds,
            vec![TerminalCommand::SetUnderlineColor(Color::Indexed(196))]
        );
    }

    #[test]
    fn parse_sgr_reset_underline_color() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[59m");
        assert_eq!(cmds, vec![TerminalCommand::ResetUnderlineColor]);
    }

    #[test]
    fn parse_sgr_inverse() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[7m");
        assert_eq!(cmds, vec![TerminalCommand::SetInverse]);
    }

    #[test]
    fn parse_sgr_hidden() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[8m");
        assert_eq!(cmds, vec![TerminalCommand::SetHidden]);
    }

    #[test]
    fn parse_sgr_strikethrough() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[9m");
        assert_eq!(cmds, vec![TerminalCommand::SetStrikethrough]);
    }

    #[test]
    fn parse_sgr_reset_bold() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[22m");
        assert_eq!(cmds, vec![TerminalCommand::ResetBold]);
    }

    #[test]
    fn parse_sgr_reset_italic() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[23m");
        assert_eq!(cmds, vec![TerminalCommand::ResetItalic]);
    }

    #[test]
    fn parse_sgr_reset_underline() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[24m");
        assert_eq!(cmds, vec![TerminalCommand::ResetUnderline]);
    }

    #[test]
    fn parse_sgr_reset_inverse() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[27m");
        assert_eq!(cmds, vec![TerminalCommand::ResetInverse]);
    }

    #[test]
    fn parse_sgr_reset_hidden() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[28m");
        assert_eq!(cmds, vec![TerminalCommand::ResetHidden]);
    }

    #[test]
    fn parse_sgr_reset_strikethrough() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[29m");
        assert_eq!(cmds, vec![TerminalCommand::ResetStrikethrough]);
    }

    #[test]
    fn parse_sgr_foreground_basic() {
        let mut parser = VtParser::new();
        // ESC[31m = red foreground (index 1)
        let cmds = parser.parse(b"\x1b[31m");
        assert_eq!(
            cmds,
            vec![TerminalCommand::SetForeground(Color::Indexed(1))]
        );
    }

    #[test]
    fn parse_sgr_background_basic() {
        let mut parser = VtParser::new();
        // ESC[42m = green background (index 2)
        let cmds = parser.parse(b"\x1b[42m");
        assert_eq!(
            cmds,
            vec![TerminalCommand::SetBackground(Color::Indexed(2))]
        );
    }

    #[test]
    fn parse_sgr_foreground_256() {
        let mut parser = VtParser::new();
        // ESC[38;5;196m = 256-color foreground, index 196
        let cmds = parser.parse(b"\x1b[38;5;196m");
        assert_eq!(
            cmds,
            vec![TerminalCommand::SetForeground(Color::Indexed(196))]
        );
    }

    #[test]
    fn parse_sgr_background_256() {
        let mut parser = VtParser::new();
        // ESC[48;5;21m = 256-color background, index 21
        let cmds = parser.parse(b"\x1b[48;5;21m");
        assert_eq!(
            cmds,
            vec![TerminalCommand::SetBackground(Color::Indexed(21))]
        );
    }

    #[test]
    fn parse_sgr_foreground_rgb() {
        let mut parser = VtParser::new();
        // ESC[38;2;255;128;0m = RGB foreground
        let cmds = parser.parse(b"\x1b[38;2;255;128;0m");
        assert_eq!(
            cmds,
            vec![TerminalCommand::SetForeground(Color::Rgb(Rgb::new(
                255, 128, 0
            )))]
        );
    }

    #[test]
    fn parse_sgr_background_rgb() {
        let mut parser = VtParser::new();
        // ESC[48;2;10;20;30m = RGB background
        let cmds = parser.parse(b"\x1b[48;2;10;20;30m");
        assert_eq!(
            cmds,
            vec![TerminalCommand::SetBackground(Color::Rgb(Rgb::new(
                10, 20, 30
            )))]
        );
    }

    #[test]
    fn parse_sgr_background_rgb_black_semicolon_form() {
        let mut parser = VtParser::new();
        // ESC[48;2;0;0;0m = RGB background black
        // Should not be mis-parsed as DIM + resets.
        let cmds = parser.parse(b"\x1b[48;2;0;0;0m");
        assert_eq!(
            cmds,
            vec![TerminalCommand::SetBackground(Color::Rgb(Rgb::new(
                0, 0, 0
            )))]
        );
    }

    #[test]
    fn parse_sgr_foreground_rgb_black_semicolon_form() {
        let mut parser = VtParser::new();
        // ESC[38;2;0;0;0m = RGB foreground black
        let cmds = parser.parse(b"\x1b[38;2;0;0;0m");
        assert_eq!(
            cmds,
            vec![TerminalCommand::SetForeground(Color::Rgb(Rgb::new(
                0, 0, 0
            )))]
        );
    }

    #[test]
    fn parse_sgr_foreground_rgb_black_then_bold() {
        let mut parser = VtParser::new();
        // ESC[38;2;0;0;0;1m = RGB black foreground + bold
        let cmds = parser.parse(b"\x1b[38;2;0;0;0;1m");
        assert_eq!(
            cmds,
            vec![
                TerminalCommand::SetForeground(Color::Rgb(Rgb::new(0, 0, 0))),
                TerminalCommand::SetBold,
            ]
        );
    }

    #[test]
    fn parse_sgr_background_rgb_colon_form() {
        let mut parser = VtParser::new();
        // ESC[48:2::10:20:30m = RGB background (colon form)
        let cmds = parser.parse(b"\x1b[48:2::10:20:30m");
        assert_eq!(
            cmds,
            vec![TerminalCommand::SetBackground(Color::Rgb(Rgb::new(
                10, 20, 30
            )))]
        );
    }

    #[test]
    fn parse_sgr_foreground_256_colon_form() {
        let mut parser = VtParser::new();
        // ESC[38:5:196m = 256-color foreground (colon form)
        let cmds = parser.parse(b"\x1b[38:5:196m");
        assert_eq!(
            cmds,
            vec![TerminalCommand::SetForeground(Color::Indexed(196))]
        );
    }

    #[test]
    fn parse_sgr_default_foreground() {
        let mut parser = VtParser::new();
        // ESC[39m = default foreground
        let cmds = parser.parse(b"\x1b[39m");
        assert_eq!(cmds, vec![TerminalCommand::SetForeground(Color::Default)]);
    }

    #[test]
    fn parse_sgr_default_background() {
        let mut parser = VtParser::new();
        // ESC[49m = default background
        let cmds = parser.parse(b"\x1b[49m");
        assert_eq!(cmds, vec![TerminalCommand::SetBackground(Color::Default)]);
    }

    #[test]
    fn parse_sgr_multiple_params() {
        let mut parser = VtParser::new();
        // ESC[1;31m = bold + red foreground
        let cmds = parser.parse(b"\x1b[1;31m");
        assert_eq!(
            cmds,
            vec![
                TerminalCommand::SetBold,
                TerminalCommand::SetForeground(Color::Indexed(1)),
            ]
        );
    }

    #[test]
    fn parse_sgr_bright_foreground() {
        let mut parser = VtParser::new();
        // ESC[91m = bright red foreground (index 9)
        let cmds = parser.parse(b"\x1b[91m");
        assert_eq!(
            cmds,
            vec![TerminalCommand::SetForeground(Color::Indexed(9))]
        );
    }

    #[test]
    fn parse_sgr_bright_background() {
        let mut parser = VtParser::new();
        // ESC[102m = bright green background (index 10)
        let cmds = parser.parse(b"\x1b[102m");
        assert_eq!(
            cmds,
            vec![TerminalCommand::SetBackground(Color::Indexed(10))]
        );
    }

    // --- Erase sequences ---

    #[test]
    fn parse_erase_in_line() {
        let mut parser = VtParser::new();
        // ESC[K = erase to end of line (mode 0)
        let cmds = parser.parse(b"\x1b[K");
        assert_eq!(cmds, vec![TerminalCommand::EraseInLine(0)]);
    }

    #[test]
    fn parse_erase_in_line_mode1() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[1K");
        assert_eq!(cmds, vec![TerminalCommand::EraseInLine(1)]);
    }

    #[test]
    fn parse_erase_in_display() {
        let mut parser = VtParser::new();
        // ESC[2J = erase entire display
        let cmds = parser.parse(b"\x1b[2J");
        assert_eq!(cmds, vec![TerminalCommand::EraseInDisplay(2)]);
    }

    // --- Delete Characters (DCH) ---

    #[test]
    fn parse_delete_chars() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[2P");
        assert_eq!(cmds, vec![TerminalCommand::DeleteChars(2)]);
    }

    #[test]
    fn parse_delete_chars_default() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[P");
        assert_eq!(cmds, vec![TerminalCommand::DeleteChars(1)]);
    }

    // --- DECTCEM (cursor visibility) ---

    #[test]
    fn parse_hide_cursor() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[?25l");
        assert_eq!(cmds, vec![TerminalCommand::HideCursor]);
    }

    #[test]
    fn parse_show_cursor() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[?25h");
        assert_eq!(cmds, vec![TerminalCommand::ShowCursor]);
    }

    // --- Alternate Screen Buffer ---

    #[test]
    fn parse_enter_alt_screen() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[?1049h");
        assert_eq!(cmds, vec![TerminalCommand::EnterAltScreen]);
    }

    #[test]
    fn parse_leave_alt_screen() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[?1049l");
        assert_eq!(cmds, vec![TerminalCommand::LeaveAltScreen]);
    }

    // --- Scroll Region (DECSTBM) ---

    #[test]
    fn parse_set_scroll_region() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[5;20r");
        assert_eq!(
            cmds,
            vec![TerminalCommand::SetScrollRegion { top: 5, bottom: 20 }]
        );
    }

    #[test]
    fn parse_reset_scroll_region() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[r");
        assert_eq!(
            cmds,
            vec![TerminalCommand::SetScrollRegion { top: 0, bottom: 0 }]
        );
    }

    // --- Insert/Delete Lines ---

    #[test]
    fn parse_insert_lines() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[3L");
        assert_eq!(cmds, vec![TerminalCommand::InsertLines(3)]);
    }

    #[test]
    fn parse_insert_lines_default() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[L");
        assert_eq!(cmds, vec![TerminalCommand::InsertLines(1)]);
    }

    #[test]
    fn parse_delete_lines() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[2M");
        assert_eq!(cmds, vec![TerminalCommand::DeleteLines(2)]);
    }

    #[test]
    fn parse_delete_lines_default() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[M");
        assert_eq!(cmds, vec![TerminalCommand::DeleteLines(1)]);
    }

    // --- Scroll Up/Down ---

    #[test]
    fn parse_scroll_up() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[2S");
        assert_eq!(cmds, vec![TerminalCommand::ScrollUp(2)]);
    }

    #[test]
    fn parse_scroll_down() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[3T");
        assert_eq!(cmds, vec![TerminalCommand::ScrollDown(3)]);
    }

    // --- Cursor Column (CHA) / Cursor Row (VPA) ---

    #[test]
    fn parse_cursor_column() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[10G");
        assert_eq!(cmds, vec![TerminalCommand::CursorColumn(10)]);
    }

    #[test]
    fn parse_cursor_column_default() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[G");
        assert_eq!(cmds, vec![TerminalCommand::CursorColumn(1)]);
    }

    #[test]
    fn parse_cursor_row() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[5d");
        assert_eq!(cmds, vec![TerminalCommand::CursorRow(5)]);
    }

    // --- Insert/Erase Characters ---

    #[test]
    fn parse_insert_chars() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[3@");
        assert_eq!(cmds, vec![TerminalCommand::InsertChars(3)]);
    }

    #[test]
    fn parse_erase_chars() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"\x1b[4X");
        assert_eq!(cmds, vec![TerminalCommand::EraseChars(4)]);
    }

    // --- Mixed content ---

    #[test]
    fn parse_text_with_newline() {
        let mut parser = VtParser::new();
        let cmds = parser.parse(b"AB\r\nCD");
        assert_eq!(
            cmds,
            vec![
                TerminalCommand::Print('A'),
                TerminalCommand::Print('B'),
                TerminalCommand::CarriageReturn,
                TerminalCommand::Newline,
                TerminalCommand::Print('C'),
                TerminalCommand::Print('D'),
            ]
        );
    }

    #[test]
    fn parse_colored_text() {
        let mut parser = VtParser::new();
        // red "Hi" then reset
        let cmds = parser.parse(b"\x1b[31mHi\x1b[0m");
        assert_eq!(
            cmds,
            vec![
                TerminalCommand::SetForeground(Color::Indexed(1)),
                TerminalCommand::Print('H'),
                TerminalCommand::Print('i'),
                TerminalCommand::ResetAttributes,
            ]
        );
    }

    // --- Partial/split sequences ---

    #[test]
    fn parse_split_escape_sequence() {
        let mut parser = VtParser::new();
        // Split ESC[31m across two chunks
        let cmds1 = parser.parse(b"\x1b[3");
        assert!(
            cmds1.is_empty(),
            "partial sequence should produce no commands"
        );

        let cmds2 = parser.parse(b"1m");
        assert_eq!(
            cmds2,
            vec![TerminalCommand::SetForeground(Color::Indexed(1))]
        );
    }

    #[test]
    fn parse_split_text_and_escape() {
        let mut parser = VtParser::new();
        let cmds1 = parser.parse(b"AB\x1b");
        assert_eq!(
            cmds1,
            vec![TerminalCommand::Print('A'), TerminalCommand::Print('B'),]
        );

        let cmds2 = parser.parse(b"[1m");
        assert_eq!(cmds2, vec![TerminalCommand::SetBold]);
    }

    // --- Unicode ---

    #[test]
    fn parse_unicode_text() {
        let mut parser = VtParser::new();
        let cmds = parser.parse("한글".as_bytes());
        assert_eq!(
            cmds,
            vec![TerminalCommand::Print('한'), TerminalCommand::Print('글'),]
        );
    }

    /// UTF-8 바이트가 분할되어 들어와도 올바르게 파싱되는지 확인
    #[test]
    fn parse_unicode_split_bytes() {
        let mut parser = VtParser::new();
        let bytes = "한".as_bytes(); // 0xED, 0x95, 0x9C
        assert_eq!(bytes, &[0xED, 0x95, 0x9C]);

        // 바이트를 하나씩 전달
        let cmds1 = parser.parse(&bytes[..1]); // 0xED
        let cmds2 = parser.parse(&bytes[1..2]); // 0x95
        let cmds3 = parser.parse(&bytes[2..3]); // 0x9C

        let all: Vec<_> = [cmds1, cmds2, cmds3].concat();
        assert_eq!(
            all,
            vec![TerminalCommand::Print('한')],
            "split UTF-8 bytes should produce the same result, got: {all:?}"
        );
    }
}
