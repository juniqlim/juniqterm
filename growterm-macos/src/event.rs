/// macOS 윈도우에서 발생하는 이벤트
#[derive(Debug, Clone)]
pub enum AppEvent {
    /// insertText: — 조합 완료 텍스트, PTY에 전송
    TextCommit(String),
    /// setMarkedText: — 조합 중 표시
    Preedit(String),
    /// doCommandBySelector: — IME가 패스한 키, 앱이 직접 처리
    KeyInput { keycode: u16, characters: Option<String>, modifiers: Modifiers },
    /// 윈도우 리사이즈
    Resize(u32, u32),
    /// 윈도우 닫기 요청
    CloseRequested,
    /// 리드로우 요청
    RedrawRequested,
    /// 마우스 버튼 누름 (x, y in backing pixels, modifiers)
    MouseDown(f64, f64, Modifiers),
    /// 마우스 드래그 (x, y in backing pixels)
    MouseDragged(f64, f64),
    /// 마우스 버튼 뗌 (x, y in backing pixels)
    MouseUp(f64, f64),
    /// 마우스 스크롤 (delta_y: 양수=위, 음수=아래)
    ScrollWheel(f64),
    /// 마우스 이동 (x, y in backing pixels, modifiers)
    MouseMoved(f64, f64, Modifiers),
    /// 파일 드래그 앤 드롭 (파일 경로 목록)
    FileDropped(Vec<String>),
    /// 뽀모도로 타이머 토글
    TogglePomodoro,
    /// 응답 타이머 토글
    ToggleResponseTimer,
    /// AI 코칭 토글
    ToggleCoaching,
    /// 반투명 탭바 토글
    ToggleTransparentTabBar,
    /// 설정 파일 리로드
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
