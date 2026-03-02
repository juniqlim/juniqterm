<p align="center">
  <img src="assets/banner.svg" alt="growTerm banner" width="100%"/>
</p>

[한국어](README.ko.md)

A terminal app that grows — GPU-accelerated terminal emulator written in Rust for macOS.

## Design Goals

- **Modular**: Each module has a single responsibility. You don't need to know the VT parser to fix clipboard copy.
- **Testable**: Pure functions and state machines are verified with unit tests; module interactions with integration tests.
- **Evolvable**: Reversible structure makes it safe to change, grow, and evolve.

## Features

- **GPU Rendering** — wgpu-based 2-pass rendering (background + glyphs)
- **Korean Support** — IME input with preedit overlay, wide character handling, D2Coding font
- **Tabs** — Cmd+T/W to open/close, Cmd+1-9 to switch, Cmd+Shift+[/] to cycle, click tab bar, new tabs inherit working directory
- **VT Parsing** — SGR attributes (bold, dim, italic, underline, strikethrough, inverse), 256/RGB color, cursor movement, screen clearing
- **TUI App Support** — Alternate screen, scroll regions, mouse tracking (SGR), bracketed paste, synchronized output, cursor visibility (DECTCEM)
- **Scrollback** — 10,000 line history, Cmd+PageUp/PageDown, draggable auto-hiding scrollbar
- **Copy Mode** — Vim-style copy mode (Cmd+Shift+C) with hjkl navigation
- **Mouse Selection & Clipboard** — Drag selection with wide character awareness, Cmd+C/V, Cmd+A to copy input line
- **URL Highlight** — Cmd+hover to underline and detect URLs
- **Pomodoro Timer** — 25min work / 3min break cycle with input blocking
- **Response Timer** — Per-tab command response time measurement
- **Font Zoom** — Cmd+=/- to adjust size (8pt–72pt)
- **Box Drawing** — Light, heavy, double, and rounded corner characters with geometric rendering
- **Keyboard** — xterm-style encoding, Shift/Ctrl/Alt modifier combinations, kitty keyboard protocol

## Architecture

```
Key Input → Input Encoding → PTY
                              ↓
                           VT Parser
                              ↓
                             Grid
                              ↓
                        Render Commands
                              ↓
                         GPU Rendering → Screen
```

### Shared Types
Data types shared by all modules (`Cell`, `Color`, `KeyEvent`, etc.). A common language so modules can talk to each other.

### Input Encoding
Translates keystrokes into bytes the shell understands.

`Ctrl+C → \x03` · `Arrow Up → \x1b[A`

### PTY
PTY (Pseudo-Terminal) is a pipe between our app and the shell. Fools the shell into thinking it's connected to a real terminal.

`\x03 → shell → \x1b[31mHello`

### VT Parser
Parses the raw bytes from the shell into structured commands.

`\x1b[31mHi → [SetColor(Red), Print('H'), Print('i')]`

### Grid
A 2D grid of cells, like a spreadsheet. Stores each character with its position and style. Also keeps scrollback history.

`[SetColor(Red), Print('H')] → grid[row=0][col=0] = 'H' (red)`

### Render Commands
Reads the grid and produces a draw list. Adds cursor, selection highlight, and IME overlay on top.

`grid[0][0]='H'(red) → DrawCell { row:0, col:0, char:'H', fg:#FF0000, bg:#000000 }`

### GPU Rendering
Takes the draw list and paints pixels on screen using the GPU. Each character becomes a bitmap composited onto the window.

`DrawCell { char:'H', fg:#FF0000 } → pixels on screen`

### macOS
Creates the window, receives mouse/keyboard events from the OS, and handles IME (Korean input).

### App
The conductor. Connects all modules: keystrokes come in, shell output comes back, the grid updates, the screen redraws.

## Build & Run

```bash
cargo build --release
cargo run -p growterm-app
```

### Install as macOS App

```bash
./install.sh
```

Builds the release binary and installs `growTerm.app` to `/Applications`.

## Test

```bash
cargo test
```

594+ tests (unit + integration).

## Requirements

- Rust (stable)
- macOS (wgpu Metal backend)
