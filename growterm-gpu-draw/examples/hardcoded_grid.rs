use growterm_gpu_draw::GpuDrawer;
use growterm_types::{CellFlags, RenderCommand, Rgb};
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

fn build_hardcoded_commands() -> Vec<RenderCommand> {
    let white = Rgb::new(255, 255, 255);
    let black = Rgb::new(0, 0, 0);
    let green = Rgb::new(0, 200, 0);
    let red = Rgb::new(200, 60, 60);
    let blue_bg = Rgb::new(30, 30, 80);
    let empty = CellFlags::empty();

    let mut cmds = Vec::new();

    // Row 0: "Hello, growterm!" in green on dark blue
    let text = "Hello, growterm!";
    for (i, c) in text.chars().enumerate() {
        cmds.push(RenderCommand {
            col: i as u16,
            row: 0,
            character: c,
            fg: green,
            bg: blue_bg,
            underline_color: None,
            flags: empty,
        });
    }

    // Row 1: "ABCDEFGHIJ" in white on black
    for i in 0..10u16 {
        cmds.push(RenderCommand {
            col: i,
            row: 1,
            character: (b'A' + i as u8) as char,
            fg: white,
            bg: black,
            underline_color: None,
            flags: empty,
        });
    }

    // Row 2: "한글 테스트" in red on black (wide chars take 2 columns)
    let korean = "한글 테스트";
    let mut col = 0u16;
    for c in korean.chars() {
        let wide = c > '\u{FF}' && c != ' ';
        cmds.push(RenderCommand {
            col,
            row: 2,
            character: c,
            fg: red,
            bg: black,
            underline_color: None,
            flags: if wide { CellFlags::WIDE_CHAR } else { empty },
        });
        col += if wide { 2 } else { 1 };
    }

    // Row 3: Bold text
    let bold_text = "BOLD";
    for (i, c) in bold_text.chars().enumerate() {
        cmds.push(RenderCommand {
            col: i as u16,
            row: 3,
            character: c,
            fg: white,
            bg: black,
            underline_color: None,
            flags: CellFlags::BOLD,
        });
    }

    cmds
}

struct App {
    window: Option<Arc<Window>>,
    drawer: Option<GpuDrawer>,
    commands: Vec<RenderCommand>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("growterm - hardcoded grid")
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
                    drawer.draw(&self.commands, None, None, false, None, false, 0.0, 0.0, 0.0);
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
        commands: build_hardcoded_commands(),
    };
    event_loop.run_app(&mut app).unwrap();
}
