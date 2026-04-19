use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{ModifiersState, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};

use crate::event::{AppEvent, Modifiers};
use crate::key_convert::physical_keycode_to_app_keycode;

pub struct MacWindow {
    window: Window,
    copy_mode: AtomicBool,
}

impl MacWindow {
    fn new(window: Window) -> Self {
        Self {
            window,
            copy_mode: AtomicBool::new(false),
        }
    }

    pub fn inner_size(&self) -> (u32, u32) {
        let size = self.window.inner_size();
        (size.width.max(1), size.height.max(1))
    }

    pub fn backing_scale_factor(&self) -> f64 {
        self.window.scale_factor()
    }

    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    pub fn set_title(&self, title: &str) {
        self.window.set_title(title);
    }

    pub fn set_copy_mode(&self, enabled: bool) {
        self.copy_mode.store(enabled, Ordering::Relaxed);
    }

    pub fn set_ime_cursor_rect(&self, _rect: Option<(f32, f32, f32, f32)>) {}

    pub fn discard_marked_text(&self) {}

    pub fn set_pomodoro_checked(&self, _checked: bool) {}

    pub fn set_response_timer_checked(&self, _checked: bool) {}

    pub fn set_coaching_checked(&self, _checked: bool) {}

    pub fn set_coaching_menu_enabled(&self, _enabled: bool) {}

    pub fn set_transparent_tab_bar_checked(&self, _checked: bool) {}

    pub fn set_transparent_mode(&self, _enabled: bool) {}

    pub fn title_bar_height(&self) -> f64 {
        0.0
    }

    pub fn set_position(&self, x: f64, y: f64) {
        self.window
            .set_outer_position(PhysicalPosition::new(x as i32, y as i32));
    }

    pub fn show(&self) {
        self.window.set_visible(true);
    }

    pub fn set_pointing_hand_cursor(&self, _enabled: bool) {}

    fn copy_mode_enabled(&self) -> bool {
        self.copy_mode.load(Ordering::Relaxed)
    }
}

unsafe impl Send for MacWindow {}
unsafe impl Sync for MacWindow {}

impl HasWindowHandle for MacWindow {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        self.window.window_handle()
    }
}

impl HasDisplayHandle for MacWindow {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        self.window.display_handle()
    }
}

struct GrowtermApp<F>
where
    F: FnOnce(Arc<MacWindow>, mpsc::Receiver<AppEvent>) + 'static,
{
    setup: Option<F>,
    window_size: (f64, f64),
    window_position: Option<(f64, f64)>,
    window: Option<Arc<MacWindow>>,
    sender: Option<mpsc::Sender<AppEvent>>,
    modifiers: ModifiersState,
    cursor_position: (f64, f64),
}

impl<F> GrowtermApp<F>
where
    F: FnOnce(Arc<MacWindow>, mpsc::Receiver<AppEvent>) + 'static,
{
    fn send(&self, event: AppEvent) {
        if let Some(sender) = &self.sender {
            let _ = sender.send(event);
        }
    }

    fn current_modifiers(&self) -> Modifiers {
        convert_modifiers(self.modifiers)
    }
}

impl<F> ApplicationHandler for GrowtermApp<F>
where
    F: FnOnce(Arc<MacWindow>, mpsc::Receiver<AppEvent>) + 'static,
{
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let mut attrs = WindowAttributes::default()
            .with_title("growterm")
            .with_inner_size(LogicalSize::new(self.window_size.0, self.window_size.1))
            .with_visible(false);
        if let Some((x, y)) = self.window_position {
            attrs = attrs.with_position(LogicalPosition::new(x, y));
        }

        let window = Arc::new(MacWindow::new(
            event_loop
                .create_window(attrs)
                .expect("create linux window"),
        ));
        let (tx, rx) = mpsc::channel();
        self.sender = Some(tx);

        if let Some(setup) = self.setup.take() {
            setup(window.clone(), rx);
        }

        window.show();
        window.request_redraw();
        self.window = Some(window);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                self.send(AppEvent::CloseRequested);
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                self.send(AppEvent::Resize(size.width.max(1), size.height.max(1)));
            }
            WindowEvent::RedrawRequested => {
                self.send(AppEvent::RedrawRequested);
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let event_type = match event.state {
                    ElementState::Pressed if event.repeat => growterm_types::KeyEventType::Repeat,
                    ElementState::Pressed => growterm_types::KeyEventType::Press,
                    ElementState::Released => growterm_types::KeyEventType::Release,
                };
                let keycode = match event.physical_key {
                    PhysicalKey::Code(code) => physical_keycode_to_app_keycode(code),
                    PhysicalKey::Unidentified(_) => None,
                };
                let characters = event.text.as_ref().map(|text| text.to_string());
                let modifiers = self.current_modifiers();
                let should_commit_text = event_type == growterm_types::KeyEventType::Press
                    && !self
                        .window
                        .as_ref()
                        .map(|window| window.copy_mode_enabled())
                        .unwrap_or(false)
                    && !modifiers
                        .intersects(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER)
                    && characters
                        .as_deref()
                        .map(|text| text.chars().all(|c| !c.is_control()))
                        .unwrap_or(false);

                if should_commit_text {
                    if let Some(text) = characters {
                        self.send(AppEvent::TextCommit(text));
                    }
                    return;
                }

                if let Some(keycode) = keycode {
                    self.send(AppEvent::KeyInput {
                        keycode,
                        characters,
                        modifiers,
                        event_type,
                    });
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_position = (position.x, position.y);
                self.send(AppEvent::MouseMoved(
                    position.x,
                    position.y,
                    self.current_modifiers(),
                ));
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if button == MouseButton::Left {
                    let (x, y) = self.cursor_position;
                    match state {
                        ElementState::Pressed => {
                            self.send(AppEvent::MouseDown(x, y, self.current_modifiers()));
                        }
                        ElementState::Released => {
                            self.send(AppEvent::MouseUp(x, y));
                        }
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let delta_y = match delta {
                    MouseScrollDelta::LineDelta(_, y) => f64::from(y) * 40.0,
                    MouseScrollDelta::PixelDelta(pos) => pos.y,
                };
                self.send(AppEvent::ScrollWheel(delta_y));
            }
            WindowEvent::DroppedFile(path) => {
                self.send(AppEvent::FileDropped(vec![path
                    .to_string_lossy()
                    .to_string()]));
            }
            _ => {}
        }
    }
}

pub fn run(
    window_size: (f64, f64),
    window_position: Option<(f64, f64)>,
    setup: impl FnOnce(Arc<MacWindow>, mpsc::Receiver<AppEvent>) + 'static,
) {
    let event_loop = EventLoop::new().expect("create linux event loop");
    let mut app = GrowtermApp {
        setup: Some(setup),
        window_size,
        window_position,
        window: None,
        sender: None,
        modifiers: ModifiersState::empty(),
        cursor_position: (0.0, 0.0),
    };
    event_loop.run_app(&mut app).expect("run linux event loop");
}

fn convert_modifiers(modifiers: ModifiersState) -> Modifiers {
    let mut out = Modifiers::empty();
    if modifiers.shift_key() {
        out |= Modifiers::SHIFT;
    }
    if modifiers.control_key() {
        out |= Modifiers::CONTROL;
    }
    if modifiers.alt_key() {
        out |= Modifiers::ALT;
    }
    if modifiers.super_key() {
        out |= Modifiers::SUPER;
    }
    out
}
