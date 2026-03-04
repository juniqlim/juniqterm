use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::mpsc::Sender;

use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSBackingStoreType, NSColor, NSWindow, NSWindowStyleMask};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use crate::dispatch::dispatch_async_main;

extern "C" fn set_needs_display_on_main(ctx: *mut c_void) {
    unsafe {
        let view: *mut objc2::runtime::AnyObject = ctx as *mut _;
        let _: () = objc2::msg_send![view, setNeedsDisplay: true];
    }
}
use raw_window_handle::{
    AppKitDisplayHandle, AppKitWindowHandle, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle,
};

use crate::event::AppEvent;
use crate::view::TerminalView;

pub struct MacWindow {
    ns_window: Retained<NSWindow>,
    view: Retained<TerminalView>,
}

impl MacWindow {
    pub fn new(mtm: MainThreadMarker, title: &str, width: f64, height: f64, position: Option<(f64, f64)>) -> Self {
        let content_rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, height));
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;

        let ns_window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                content_rect,
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };

        let view = TerminalView::new(mtm);

        ns_window.setTitlebarAppearsTransparent(true);
        ns_window.setBackgroundColor(Some(&NSColor::blackColor()));
        ns_window.setTabbingMode(objc2_app_kit::NSWindowTabbingMode::Disallowed);
        ns_window.setContentView(Some(&view));
        ns_window.makeFirstResponder(Some(&view));

        let title_str = NSString::from_str(title);
        ns_window.setTitle(&title_str);
        if position.is_none() {
            ns_window.center();
        }

        Self { ns_window, view }
    }

    pub fn set_sender(&self, sender: Sender<AppEvent>) {
        self.view.set_sender(sender);
    }

    pub fn inner_size(&self) -> (u32, u32) {
        let frame = self.view.frame();
        let scale = self.backing_scale_factor();
        let w = (frame.size.width * scale) as u32;
        let h = (frame.size.height * scale) as u32;
        (w.max(1), h.max(1))
    }

    pub fn backing_scale_factor(&self) -> f64 {
        self.ns_window.backingScaleFactor()
    }

    pub fn request_redraw(&self) {
        let ptr = Retained::as_ptr(&self.view) as *mut c_void;
        unsafe {
            crate::dispatch::dispatch_raw_main(ptr, set_needs_display_on_main);
        }
    }

    pub fn set_title(&self, title: &str) {
        let raw: *const NSWindow = Retained::as_ptr(&self.ns_window);
        let title = title.to_owned();
        unsafe {
            let raw = raw as usize;
            dispatch_async_main(move || {
                let window = raw as *const NSWindow;
                let ns_title = NSString::from_str(&title);
                (*window).setTitle(&ns_title);
            });
        }
    }

    pub fn set_copy_mode(&self, enabled: bool) {
        let raw = Retained::as_ptr(&self.view) as usize;
        dispatch_async_main(move || {
            let view = unsafe { &*(raw as *const TerminalView) };
            view.set_copy_mode_bypass_ime(enabled);
        });
    }

    pub fn discard_marked_text(&self) {
        let raw = Retained::as_ptr(&self.view) as usize;
        dispatch_async_main(move || {
            let view = raw as *const objc2::runtime::AnyObject;
            unsafe {
                let ctx: *const objc2::runtime::AnyObject = objc2::msg_send![view, inputContext];
                if !ctx.is_null() {
                    let _: () = objc2::msg_send![ctx, discardMarkedText];
                }
            }
        });
    }

    pub fn set_pomodoro_checked(&self, checked: bool) {
        set_view_menu_item_checked(0, checked);
    }

    pub fn set_response_timer_checked(&self, checked: bool) {
        set_view_menu_item_checked(1, checked);
    }

    pub fn set_coaching_checked(&self, checked: bool) {
        set_view_menu_item_checked(2, checked);
    }

    pub fn set_coaching_menu_enabled(&self, enabled: bool) {
        set_view_menu_item_enabled(2, enabled);
    }

    pub fn set_transparent_tab_bar_checked(&self, checked: bool) {
        set_view_menu_item_checked(3, checked);
    }

    pub fn set_transparent_mode(&self, enabled: bool) {
        let raw = Retained::as_ptr(&self.ns_window) as usize;
        dispatch_async_main(move || {
            let window = unsafe { &*(raw as *const NSWindow) };
            let mut style = window.styleMask();
            if enabled {
                style |= NSWindowStyleMask::FullSizeContentView;
            } else {
                style -= NSWindowStyleMask::FullSizeContentView;
            }
            let frame = window.frame();
            window.setStyleMask(style);
            window.setFrame_display(frame, true);
        });
    }

    pub fn title_bar_height(&self) -> f64 {
        let frame = self.ns_window.frame();
        let content_rect = self.ns_window.contentLayoutRect();
        (frame.size.height - content_rect.size.height) * self.backing_scale_factor()
    }

    /// 화면 좌상단 기준 좌표(x, y)로 윈도우를 이동.
    /// macOS 좌표계(좌하단 원점)로 변환하여 적용.
    pub fn set_position(&self, x: f64, y: f64) {
        let screen_height = self.ns_window.screen()
            .map(|s| s.frame().size.height)
            .unwrap_or(900.0);
        let window_height = self.ns_window.frame().size.height;
        let flipped_y = screen_height - y - window_height;
        self.ns_window.setFrameOrigin(NSPoint::new(x, flipped_y));
    }

    pub fn show(&self) {
        self.ns_window.makeKeyAndOrderFront(None);
    }

    pub fn ns_window(&self) -> &NSWindow {
        &self.ns_window
    }
}

fn set_view_menu_item_checked(index: isize, checked: bool) {
    dispatch_async_main(move || {
        let mtm = MainThreadMarker::new().unwrap();
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        if let Some(menu) = app.mainMenu() {
            if let Some(view_menu_item) = menu.itemAtIndex(1) {
                if let Some(view_menu) = view_menu_item.submenu() {
                    if let Some(item) = view_menu.itemAtIndex(index) {
                        let state = if checked { 1 } else { 0 };
                        item.setState(state);
                    }
                }
            }
        }
    });
}

fn set_view_menu_item_enabled(index: isize, enabled: bool) {
    dispatch_async_main(move || {
        let mtm = MainThreadMarker::new().unwrap();
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        if let Some(menu) = app.mainMenu() {
            if let Some(view_menu_item) = menu.itemAtIndex(1) {
                if let Some(view_menu) = view_menu_item.submenu() {
                    if let Some(item) = view_menu.itemAtIndex(index) {
                        item.setEnabled(enabled);
                    }
                }
            }
        }
    });
}

unsafe impl Send for MacWindow {}
unsafe impl Sync for MacWindow {}

impl HasWindowHandle for MacWindow {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let view_ptr = Retained::as_ptr(&self.view) as *mut std::ffi::c_void;
        let non_null = NonNull::new(view_ptr).expect("view pointer should not be null");
        let handle = AppKitWindowHandle::new(non_null);
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(RawWindowHandle::AppKit(handle)) })
    }
}

impl HasDisplayHandle for MacWindow {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        let handle = AppKitDisplayHandle::new();
        Ok(unsafe {
            raw_window_handle::DisplayHandle::borrow_raw(RawDisplayHandle::AppKit(handle))
        })
    }
}
