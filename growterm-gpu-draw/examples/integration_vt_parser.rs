/// Integration test: bytes → VtParser → TerminalCommand → Grid → generate() → GpuDrawer::draw()
/// Phase 4 (grid) + Phase 3 (vt-parser) + Phase 2 (render-cmd) + Phase 1 (gpu-draw) 파이프라인 검증
use growterm_gpu_draw::GpuDrawer;
use growterm_grid::Grid;
use growterm_render_cmd::{generate, TerminalPalette};
use growterm_types::Cell;
use growterm_vt_parser::VtParser;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

fn build_grid_from_escapes() -> Vec<Vec<Cell>> {
    let mut parser = VtParser::new();
    let mut grid = Grid::new(80, 24);

    // 실제 터미널 이스케이프 시퀀스로 화면 구성
    let terminal_output = concat!(
        // Row 0: "growterm" in bright green, bold
        "\x1b[1;32mgrowterm\x1b[0m - VT Parser Integration Test\r\n",
        // Row 1: empty
        "\r\n",
        // Row 2: 256-color foreground
        "\x1b[38;5;196mRed 256\x1b[0m ",
        "\x1b[38;5;21mBlue 256\x1b[0m ",
        "\x1b[38;5;226mYellow 256\x1b[0m\r\n",
        // Row 3: RGB colors
        "\x1b[38;2;255;128;0mOrange RGB\x1b[0m ",
        "\x1b[38;2;128;0;255mPurple RGB\x1b[0m\r\n",
        // Row 4: Background colors
        "\x1b[41m Red BG \x1b[42m Green BG \x1b[44m Blue BG \x1b[0m\r\n",
        // Row 5: Attributes
        "\x1b[1mBOLD\x1b[0m ",
        "\x1b[2mDIM\x1b[0m ",
        "\x1b[7mINVERSE\x1b[0m ",
        "\x1b[4mUNDERLINE\x1b[0m\r\n",
        // Row 6: Combined: bold + red + blue bg
        "\x1b[1;31;44m Bold Red on Blue \x1b[0m\r\n",
        // Row 7: Cursor positioning
        "\x1b[8;1HCURSOR POS TEST",
        // Row 8: Korean text with color
        "\x1b[9;1H\x1b[33m한글 테스트\x1b[0m",
    );

    let cmds = parser.parse(terminal_output.as_bytes());
    for cmd in &cmds {
        grid.apply(cmd);
    }
    grid.cells().to_vec()
}

struct App {
    window: Option<Arc<Window>>,
    drawer: Option<GpuDrawer>,
    grid: Vec<Vec<Cell>>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("growterm - integration: bytes → VtParser → Grid → generate → draw")
            .with_inner_size(winit::dpi::LogicalSize::new(900, 500));
        let window = Arc::new(event_loop.create_window(attrs).unwrap());
        let size = window.inner_size();
        let drawer = GpuDrawer::new(window.clone(), size.width, size.height, 24.0, None);
        self.window = Some(window);
        self.drawer = Some(drawer);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(drawer) = &mut self.drawer {
                    drawer.resize(size.width, size.height);
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(drawer) = &mut self.drawer {
                    let commands =
                        generate(&self.grid, None, None, None, TerminalPalette::default());
                    drawer.draw(&commands, None, None, false);
                }
            }
            _ => {}
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let mut app = App {
        window: None,
        drawer: None,
        grid: build_grid_from_escapes(),
    };
    event_loop.run_app(&mut app).unwrap();
}
