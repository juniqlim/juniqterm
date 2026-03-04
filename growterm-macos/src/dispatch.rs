use std::ffi::c_void;

extern "C" {
    static _dispatch_main_q: c_void;
    fn dispatch_async_f(
        queue: *const c_void,
        context: *mut c_void,
        work: extern "C" fn(*mut c_void),
    );
}

extern "C" fn dispatch_trampoline(ctx: *mut c_void) {
    let closure: Box<Box<dyn FnOnce()>> = unsafe { Box::from_raw(ctx as *mut _) };
    closure();
}

/// Low-level dispatch to main queue with raw context pointer and C function.
pub unsafe fn dispatch_raw_main(context: *mut c_void, work: extern "C" fn(*mut c_void)) {
    dispatch_async_f(
        &_dispatch_main_q as *const _ as *const c_void,
        context,
        work,
    );
}

pub fn dispatch_async_main<F: FnOnce() + 'static>(f: F) {
    let boxed: Box<Box<dyn FnOnce()>> = Box::new(Box::new(f));
    let ptr = Box::into_raw(boxed) as *mut c_void;
    unsafe {
        dispatch_async_f(
            &_dispatch_main_q as *const _ as *const c_void,
            ptr,
            dispatch_trampoline,
        );
    }
}
