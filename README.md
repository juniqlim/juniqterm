<p align="center">
  <img src="assets/banner.svg" alt="growTerm banner" width="100%"/>
</p>

[한국어](README.ko.md)

A terminal app that grows — GPU-accelerated terminal emulator written in Rust for macOS. With a few fun features.

## Design Goals

- **Modular**: Each module has a single responsibility. You don't need to know the VT parser to fix clipboard copy.
- **Testable**: Pure functions and state machines are verified with unit tests; module interactions with integration tests.
- **Evolvable**: Reversible structure makes it safe to change, grow, and evolve.

## Features

- **GPU Rendering** — wgpu-based 2-pass rendering (background + glyphs)
- **Tabs** — Cmd+T/W to open/close, Cmd+1-9 to switch, Cmd+Shift+[/] to cycle, click tab bar, new tabs inherit working directory
- **VT Parsing** — SGR attributes (bold, dim, italic, underline, strikethrough, inverse), 256/RGB color, cursor movement, screen clearing
- **TUI App Support** — Alternate screen, scroll regions, mouse tracking (SGR), bracketed paste, synchronized output, cursor visibility (DECTCEM)
- **Scrollback** — 10,000 line history, Cmd+PageUp/PageDown, draggable auto-hiding scrollbar
- **Copy Mode** — Vim-style copy mode (Cmd+Shift+C) with hjkl navigation
- **Mouse Selection & Clipboard** — Drag selection with wide character awareness, Cmd+C/V, Cmd+A to copy input line
- **URL Highlight** — Cmd+hover to underline and detect URLs
- **Pomodoro Timer** — Configurable work/break cycle with input blocking (default 25min/3min)
- **Response Timer** — Per-tab command response time measurement
- **Coaching** — AI coaching layer with word wrapping (uses Claude CLI)
- **Font Zoom** — Cmd+=/- to adjust size (8pt–72pt)
- **Box Drawing** — Light, heavy, double, and rounded corner characters with geometric rendering
- **Keyboard** — xterm-style encoding, Shift/Ctrl/Alt modifier combinations, kitty keyboard protocol

## Keyboard Shortcuts

| Shortcut | Action |
|---|---|
| Cmd+N | New window |
| Cmd+T | New tab |
| Cmd+W | Close tab |
| Cmd+1–9 | Switch to tab by number |
| Cmd+Shift+[ / ] | Previous / next tab |
| Cmd+C | Copy |
| Cmd+V | Paste |
| Cmd+A | Copy input line to clipboard |
| Cmd+= / Cmd+- | Zoom in / out |
| Cmd+PageUp/Down | Scroll one page |
| Cmd+Home / End | Scroll to top / bottom |
| Cmd+Click | Open URL under cursor |
| `` ` `` or Cmd+Shift+C | Enter / exit copy mode |

### Copy Mode

| Key | Action |
|---|---|
| j / k | Move 1 line down / up |
| h / l | Move 10 lines up / down |
| v | Toggle visual mode (multi-line selection) |
| Cmd+C | Copy selection and exit copy mode |

## Configuration

Settings are stored in `~/.config/growterm/config.toml`. All fields are optional — omitted values use defaults.

```toml
font_family = "FiraCodeNerdFontMono-Retina"  # font name
font_size = 32.0                              # font size in pt
pomodoro = false                              # enable pomodoro timer
pomodoro_work_minutes = 25                    # work duration
pomodoro_break_minutes = 3                    # break duration
response_timer = false                        # enable response timer
coaching = true                               # enable AI coaching
coaching_command = "claude -p ..."            # custom coaching command
transparent_tab_bar = false                   # transparent tab/title bar
header_opacity = 0.8                          # tab bar opacity (0.0–1.0)
window_width = 800                            # initial window width
window_height = 600                           # initial window height
window_x = 100                                # window x position
window_y = 50                                 # window y position

[copy_mode_keys]
down = "j"                                    # single key or array
up = "k"
visual = "v"
half_page_down = ["h", "d"]
half_page_up = ["l", "u"]
yank = "y"
exit = ["q", "Escape", "`"]
```

Legacy individual config files (`pomodoro_enabled`, etc.) are automatically migrated to `config.toml` on first load.

### Coaching

When a pomodoro break starts, growTerm captures all terminal output from the work session and sends it to an AI for coaching feedback. The response is displayed as an overlay during the break.

**Default behavior** — Uses Claude CLI (`claude -p`) with this system prompt:

> You are a coach. Don't judge or teach. Briefly describe what you observed, and ask a question about something the user might have missed. Answer in Korean, 3–4 sentences max.

**Using a different model** — Set `coaching_command` to any shell command. The terminal output is piped to stdin.

```toml
# Example: use GPT-4o instead
coaching_command = "openai api chat.completions.create -m gpt-4o"

# Example: use a local Ollama model
coaching_command = "ollama run llama3"

# Example: use Claude with a custom prompt
coaching_command = "claude --system-prompt 'You are a concise code reviewer.' -p"
```

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

717+ tests (unit + integration).

### Install on Ubuntu UI

Linux support uses `winit` with `wgpu`'s Vulkan/OpenGL backends.

```bash
./install-ubuntu.sh
growterm
```

For a normal Ubuntu package:

```bash
./package-ubuntu-deb.sh
sudo apt install ./target/packages/growterm_0.1.0_$(dpkg --print-architecture).deb
```

See [docs/ubuntu.md](docs/ubuntu.md) for details and GNOME notes.

## Requirements

- Rust (stable)
- macOS (wgpu Metal backend)
- Ubuntu/Linux UI (experimental, wgpu Vulkan/OpenGL backend)

## License

MIT
