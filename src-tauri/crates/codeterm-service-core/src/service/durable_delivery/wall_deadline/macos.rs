use parking_lot::Mutex;
use std::ffi::c_void;

type Source = *mut c_void;
type Callback = Mutex<Option<Box<dyn FnOnce() + Send>>>;

#[link(name = "System")]
unsafe extern "C" {
    static _dispatch_source_type_timer: u8;
    fn dispatch_get_global_queue(identifier: isize, flags: usize) -> Source;
    fn dispatch_source_create(
        kind: *const c_void,
        handle: usize,
        mask: usize,
        queue: Source,
    ) -> Source;
    fn dispatch_set_context(object: Source, context: *mut c_void);
    fn dispatch_source_set_event_handler_f(source: Source, handler: extern "C" fn(*mut c_void));
    fn dispatch_source_set_cancel_handler_f(source: Source, handler: extern "C" fn(*mut c_void));
    fn dispatch_source_set_timer(source: Source, start: u64, interval: u64, leeway: u64);
    fn dispatch_walltime(when: *const libc::timespec, delta: i64) -> u64;
    fn dispatch_resume(object: Source);
    fn dispatch_source_cancel(source: Source);
    fn dispatch_release(object: Source);
}

pub(in super::super) struct WallDeadline(usize);

extern "C" fn fire(context: *mut c_void) {
    // The source retains this context until its serialized cancellation handler runs.
    let callback = unsafe { &*(context as *const Callback) }.lock().take();
    if let Some(callback) = callback {
        callback();
    }
}

extern "C" fn cancelled(context: *mut c_void) {
    // libdispatch runs cancellation after the last event handler has returned.
    drop(unsafe { Box::from_raw(context as *mut Callback) });
}

impl WallDeadline {
    pub(in super::super) fn arm(
        deadline_ms: u64,
        callback: impl FnOnce() + Send + 'static,
    ) -> Result<Self, String> {
        let seconds = i64::try_from(deadline_ms / 1000).map_err(|_| "wall deadline overflow")?;
        let when = libc::timespec {
            tv_sec: seconds,
            tv_nsec: (deadline_ms % 1000 * 1_000_000) as _,
        };
        unsafe {
            let queue = dispatch_get_global_queue(0, 0);
            let source = dispatch_source_create(
                std::ptr::addr_of!(_dispatch_source_type_timer).cast(),
                0,
                0,
                queue,
            );
            if source.is_null() {
                return Err("wall deadline source unavailable".into());
            }
            let context: Box<Callback> = Box::new(Mutex::new(Some(Box::new(callback))));
            dispatch_set_context(source, Box::into_raw(context).cast());
            dispatch_source_set_event_handler_f(source, fire);
            dispatch_source_set_cancel_handler_f(source, cancelled);
            dispatch_source_set_timer(source, dispatch_walltime(&when, 0), u64::MAX, 0);
            dispatch_resume(source);
            Ok(Self(source as usize))
        }
    }
}

impl Drop for WallDeadline {
    fn drop(&mut self) {
        unsafe {
            dispatch_source_cancel(self.0 as Source);
            dispatch_release(self.0 as Source);
        }
    }
}
