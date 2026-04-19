pub use growterm_types::KeyEventType;

#[derive(Debug, Clone)]
pub enum AppEvent {
    TextCommit(String),
    Preedit(String),
    KeyInput {
        keycode: u16,
        characters: Option<String>,
        modifiers: Modifiers,
        event_type: KeyEventType,
    },
    Resize(u32, u32),
    CloseRequested,
    RedrawRequested,
    MouseDown(f64, f64, Modifiers),
    MouseDragged(f64, f64),
    MouseUp(f64, f64),
    ScrollWheel(f64),
    MouseMoved(f64, f64, Modifiers),
    FileDropped(Vec<String>),
    TogglePomodoro,
    ToggleResponseTimer,
    ToggleCoaching,
    ToggleTransparentTabBar,
    ReloadConfig,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct Modifiers: u8 {
        const SHIFT   = 0b0001;
        const CONTROL = 0b0010;
        const ALT     = 0b0100;
        const SUPER   = 0b1000;
    }
}
