use parking_lot::Mutex;

#[doc(hidden)]
pub fn request_with_idle<C, T>(
    slot: &Mutex<Option<C>>,
    spawn: impl FnOnce() -> Result<C, String>,
    recycle: impl FnOnce(&C) -> bool,
    request: impl FnOnce(&mut C) -> Result<T, String>,
) -> Result<T, String> {
    let idle = slot.lock().take();
    let mut child = if let Some(child) = idle {
        if recycle(&child) {
            spawn()?
        } else {
            child
        }
    } else {
        spawn()?
    };

    let result = request(&mut child);
    if result.is_ok() {
        let mut guard = slot.lock();
        if guard.is_none() {
            *guard = Some(child);
        }
    }
    result
}
