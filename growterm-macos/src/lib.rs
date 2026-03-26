mod delegate;
mod dispatch;
pub mod event;
pub mod key_convert;
#[doc(hidden)]
pub mod view;
mod window;

pub use event::{AppEvent, KeyEventType, Modifiers};
pub use key_convert::convert_key;
pub use window::MacWindow;

/// 통합 테스트용 헬퍼. 프로덕션 코드에서 사용하지 않음.
#[doc(hidden)]
pub mod test_support {
    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use crate::view::TerminalView;

    pub fn create_terminal_view(mtm: MainThreadMarker) -> Retained<TerminalView> {
        TerminalView::new(mtm)
    }

    pub fn set_view_sender(view: &TerminalView, sender: std::sync::mpsc::Sender<crate::AppEvent>) {
        view.set_sender(sender);
    }

    pub fn set_copy_mode_bypass_ime(view: &TerminalView, enabled: bool) {
        view.set_copy_mode_bypass_ime(enabled);
    }

    pub fn setup_menu(app: &objc2_app_kit::NSApplication) {
        crate::setup_main_menu(app);
    }
}

use std::sync::mpsc;

use objc2::runtime::ProtocolObject;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSMenu, NSMenuItem};
use objc2_foundation::NSString;

use delegate::AppDelegate;

/// macOS 네이티브 이벤트 루프 실행.
///
/// 이 함수는 메인 스레드에서 호출되어야 하며 반환하지 않음.
/// `setup` 콜백은 applicationDidFinishLaunching에서 호출되어
/// IMK 입력 서버 연결이 완료된 상태에서 윈도우가 생성됨.
pub fn run(
    window_size: (f64, f64),
    window_position: Option<(f64, f64)>,
    setup: impl FnOnce(std::sync::Arc<MacWindow>, mpsc::Receiver<AppEvent>) + 'static,
) -> ! {
    // bare 바이너리(cargo run)에서도 IMK 입력 서버가 연결되도록
    // 번들 ID를 런타임에 설정. .app 번들로 실행 시에는 Info.plist 값이 이미 있으므로 무해.
    ensure_bundle_identifier();

    let mtm = MainThreadMarker::new().expect("must be called from main thread");

    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    setup_main_menu(&app);
    let delegate = AppDelegate::new(mtm, window_size, window_position, Box::new(setup));
    let delegate_proto: &ProtocolObject<dyn NSApplicationDelegate> =
        ProtocolObject::from_ref(&*delegate);
    app.setDelegate(Some(delegate_proto));

    app.run();

    std::process::exit(0);
}

fn setup_main_menu(app: &NSApplication) {
    let mtm = MainThreadMarker::new().unwrap();
    unsafe {
        let menubar = NSMenu::new(mtm);

        // App menu
        let app_menu_item = NSMenuItem::new(mtm);
        menubar.addItem(&app_menu_item);

        let app_menu = NSMenu::new(mtm);
        let quit_title = NSString::from_str("Quit growTerm");
        let quit_key = NSString::from_str("q");
        let quit_item = NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &quit_title,
            Some(objc2::sel!(terminate:)),
            &quit_key,
        );
        app_menu.addItem(&quit_item);
        app_menu_item.setSubmenu(Some(&app_menu));

        // View menu
        let view_menu_item = NSMenuItem::new(mtm);
        menubar.addItem(&view_menu_item);

        let view_menu = NSMenu::initWithTitle(mtm.alloc(), &NSString::from_str("View"));
        view_menu.setAutoenablesItems(false);
        let pomodoro_title = NSString::from_str("Pomodoro Timer");
        let pomodoro_key = NSString::from_str("p");
        let pomodoro_item = NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &pomodoro_title,
            Some(objc2::sel!(togglePomodoro:)),
            &pomodoro_key,
        );
        view_menu.addItem(&pomodoro_item);

        let response_timer_title = NSString::from_str("Response Timer");
        let response_timer_key = NSString::from_str("r");
        let response_timer_item = NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &response_timer_title,
            Some(objc2::sel!(toggleResponseTimer:)),
            &response_timer_key,
        );
        view_menu.addItem(&response_timer_item);

        let coaching_title = NSString::from_str("AI Coaching");
        let coaching_key = NSString::from_str("");
        let coaching_item = NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &coaching_title,
            Some(objc2::sel!(toggleCoaching:)),
            &coaching_key,
        );
        view_menu.addItem(&coaching_item);

        let transparent_tab_title = NSString::from_str("Transparent Mode");
        let transparent_tab_key = NSString::from_str("");
        let transparent_tab_item = NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &transparent_tab_title,
            Some(objc2::sel!(toggleTransparentTabBar:)),
            &transparent_tab_key,
        );
        view_menu.addItem(&transparent_tab_item);

        let separator = NSMenuItem::separatorItem(mtm);
        view_menu.addItem(&separator);

        let reload_title = NSString::from_str("Reload Config");
        let reload_key = NSString::from_str("R");
        let reload_item = NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &reload_title,
            Some(objc2::sel!(reloadConfig:)),
            &reload_key,
        );
        view_menu.addItem(&reload_item);
        view_menu_item.setSubmenu(Some(&view_menu));

        app.setMainMenu(Some(&menubar));
    }
}

/// bare 바이너리(cargo run)에서도 IMK 입력 서버가 연결되도록
/// 실행 파일을 감싸는 임시 .app 번들 구조를 만들고,
/// 실행 파일을 심볼릭 링크로 연결한다.
/// .app 번들로 실행 시에는 아무 작업도 하지 않는다.
fn ensure_bundle_identifier() {
    if should_skip_bundle_relaunch() {
        return;
    }

    let Ok(exe) = std::env::current_exe() else { return };
    let exe = exe.canonicalize().unwrap_or(exe);
    let Some(dir) = exe.parent() else { return };

    // 이미 .app 번들 내부면 skip
    if dir.to_string_lossy().contains(".app/") {
        return;
    }

    let exe_name = exe.file_name().unwrap().to_string_lossy();
    let app_dir = dir.join(format!("{exe_name}.app"));
    let macos_dir = app_dir.join("Contents/MacOS");
    let plist_path = app_dir.join("Contents/Info.plist");
    let link_path = macos_dir.join(exe_name.as_ref());

    if plist_path.exists() && link_path.exists() {
        // 바이너리가 갱신되었으면 복사본도 갱신
        let src_modified = std::fs::metadata(&exe).and_then(|m| m.modified()).ok();
        let dst_modified = std::fs::metadata(&link_path).and_then(|m| m.modified()).ok();
        match (src_modified, dst_modified) {
            (Some(src), Some(dst)) if src <= dst => return,
            _ => {}
        }
    }

    let _ = std::fs::create_dir_all(&macos_dir);

    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>{exe_name}</string>
    <key>CFBundleIdentifier</key>
    <string>com.juniqlim.growterm</string>
    <key>CFBundleName</key>
    <string>growTerm</string>
</dict>
</plist>"#
    );
    let _ = std::fs::write(&plist_path, content);

    // 바이너리를 .app 번들에 복사 (심볼릭 링크 대신)
    let _ = std::fs::remove_file(&link_path);
    let _ = std::fs::copy(&exe, &link_path);

    // `open` 명령으로 .app 번들을 실행 (Launch Services 등록 필요)
    let _ = std::process::Command::new("open")
        .arg("-n")
        .arg(&app_dir)
        .arg("--args")
        .args(std::env::args_os().skip(1))
        .spawn();

    std::process::exit(0);
}

fn should_skip_bundle_relaunch() -> bool {
    matches!(
        std::env::var("GROWTERM_DISABLE_APP_RELAUNCH"),
        Ok(v) if !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_skip_bundle_relaunch_when_env_is_set() {
        unsafe { std::env::set_var("GROWTERM_DISABLE_APP_RELAUNCH", "1") };
        assert!(should_skip_bundle_relaunch());
        unsafe { std::env::remove_var("GROWTERM_DISABLE_APP_RELAUNCH") };
    }

    #[test]
    fn should_not_skip_bundle_relaunch_when_env_is_unset() {
        unsafe { std::env::remove_var("GROWTERM_DISABLE_APP_RELAUNCH") };
        assert!(!should_skip_bundle_relaunch());
    }
}
