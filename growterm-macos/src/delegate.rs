use std::cell::RefCell;
use std::sync::{mpsc, Arc};

use objc2::rc::Retained;
use objc2::{define_class, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSApplicationDelegate};
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol};

use crate::event::AppEvent;
use crate::window::MacWindow;

type SetupFn = Box<dyn FnOnce(Arc<MacWindow>, mpsc::Receiver<AppEvent>) + 'static>;

pub(crate) struct DelegateIvars {
    setup: RefCell<Option<SetupFn>>,
    window_size: (f64, f64),
    window_position: Option<(f64, f64)>,
}

define_class! {
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "GrowAppDelegate"]
    #[ivars = DelegateIvars]
    pub(crate) struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _notification: &NSNotification) {
            let mtm = MainThreadMarker::new().unwrap();
            let app = NSApplication::sharedApplication(mtm);
            app.activate();

            // 윈도우 생성을 다음 런루프 틱으로 지연.
            // didFinishLaunching 시점에는 IMK 입력 서버의 mach port 연결이
            // 아직 완료되지 않아, 즉시 윈도우를 만들면 자소 분리가 발생함.
            let setup = self.ivars().setup.borrow_mut().take();
            let (w, h) = self.ivars().window_size;
            let pos = self.ivars().window_position;
            if let Some(setup) = setup {
                dispatch_async_main(move || {
                    let mtm = MainThreadMarker::new().unwrap();
                    let mac_window = MacWindow::new(mtm, "growterm", w, h, pos);
                    let (tx, rx) = mpsc::channel();
                    mac_window.set_sender(tx);
                    mac_window.show();
                    if let Some((x, y)) = pos {
                        mac_window.set_position(x, y);
                    }

                    let mac_window = Arc::new(mac_window);
                    setup(mac_window, rx);
                });
            }
        }

        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn should_terminate_after_last_window_closed(&self, _app: &NSApplication) -> bool {
            true
        }
    }
}

use crate::dispatch::dispatch_async_main;

impl AppDelegate {
    pub(crate) fn new(mtm: MainThreadMarker, window_size: (f64, f64), window_position: Option<(f64, f64)>, setup: SetupFn) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(DelegateIvars {
            setup: RefCell::new(Some(setup)),
            window_size,
            window_position,
        });
        unsafe { objc2::msg_send![super(this), init] }
    }
}

