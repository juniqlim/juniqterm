/// Integration test: Cell → render-cmd::generate() → GpuDrawer::draw()
/// Phase 0 (types) + Phase 2 (render-cmd) + Phase 1 (gpu-draw) 파이프라인 검증
use growterm_gpu_draw::GpuDrawer;
use growterm_render_cmd::{generate, TerminalPalette};
use growterm_types::{Cell, CellFlags, Color, Rgb};
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

fn build_grid() -> Vec<Vec<Cell>> {
    let mut grid = Vec::new();

    // Row 0: "Hello" in green RGB
    let hello: Vec<Cell> = "Hello, growterm!"
        .chars()
        .map(|c| Cell {
            character: c,
            fg: Color::Rgb(Rgb::new(0, 200, 0)),
            bg: Color::Rgb(Rgb::new(30, 30, 80)),
            flags: CellFlags::empty(),
        })
        .collect();
    grid.push(hello);

    // Row 1: ANSI indexed colors (0~7)
    let ansi: Vec<Cell> = (0..8u8)
        .map(|i| Cell {
            character: (b'0' + i) as char,
            fg: Color::Indexed(15), // bright white
            bg: Color::Indexed(i),
            flags: CellFlags::empty(),
        })
        .collect();
    grid.push(ansi);

    // Row 2: Korean wide chars
    let mut korean_cells = Vec::new();
    for c in "한글 테스트".chars() {
        let wide = c > '\u{FF}' && c != ' ';
        korean_cells.push(Cell {
            character: c,
            fg: Color::Indexed(1), // red
            bg: Color::Default,
            flags: if wide {
                CellFlags::WIDE_CHAR
            } else {
                CellFlags::empty()
            },
        });
    }
    grid.push(korean_cells);

    // Row 3: INVERSE test
    let inverse: Vec<Cell> = "INVERSE"
        .chars()
        .map(|c| Cell {
            character: c,
            fg: Color::Rgb(Rgb::new(255, 255, 255)),
            bg: Color::Rgb(Rgb::new(0, 0, 0)),
            flags: CellFlags::INVERSE,
        })
        .collect();
    grid.push(inverse);

    // Row 4: DIM test
    let dim: Vec<Cell> = "DIM TEXT"
        .chars()
        .map(|c| Cell {
            character: c,
            fg: Color::Rgb(Rgb::new(200, 200, 200)),
            bg: Color::Default,
            flags: CellFlags::DIM,
        })
        .collect();
    grid.push(dim);

    grid
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
            .with_title("growterm - integration: Cell → generate → draw")
            .with_inner_size(winit::dpi::LogicalSize::new(800, 400));
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
                    // Integration pipeline: Cell → generate() → draw()
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
        grid: build_grid(),
    };
    event_loop.run_app(&mut app).unwrap();
}
