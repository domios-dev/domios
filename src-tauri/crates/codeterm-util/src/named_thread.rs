use std::thread::{Builder, JoinHandle};

pub fn spawn<F, T>(name: impl Into<String>, work: F) -> JoinHandle<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    Builder::new()
        .name(name.into())
        .spawn(work)
        .unwrap_or_else(|error| panic!("failed to spawn named thread: {error}"))
}

pub struct Scope {
    previous: String,
    _thread_bound: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl Scope {
    pub fn enter(name: &str) -> Self {
        let previous = native::get();
        native::set(name);
        Self {
            previous,
            _thread_bound: std::marker::PhantomData,
        }
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        native::set(&self.previous);
    }
}

#[cfg(windows)]
mod native {
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentThread() -> *mut c_void;
        fn GetThreadDescription(thread: *mut c_void, description: *mut *mut u16) -> i32;
        fn SetThreadDescription(thread: *mut c_void, description: *const u16) -> i32;
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }

    pub(super) fn get() -> String {
        unsafe {
            let mut text = std::ptr::null_mut();
            if GetThreadDescription(GetCurrentThread(), &mut text) < 0 || text.is_null() {
                return String::new();
            }
            let mut len = 0;
            while *text.add(len) != 0 {
                len += 1;
            }
            let name = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
            LocalFree(text.cast());
            name
        }
    }

    pub(super) fn set(name: &str) {
        let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        unsafe {
            SetThreadDescription(GetCurrentThread(), name.as_ptr());
        }
    }
}

#[cfg(unix)]
mod native {
    pub(super) fn get() -> String {
        let mut name = [0 as libc::c_char; 64];
        unsafe {
            if libc::pthread_getname_np(libc::pthread_self(), name.as_mut_ptr(), name.len()) != 0 {
                return String::new();
            }
            std::ffi::CStr::from_ptr(name.as_ptr())
                .to_string_lossy()
                .into_owned()
        }
    }

    pub(super) fn set(name: &str) {
        let limit = if cfg!(target_os = "linux") { 15 } else { 63 };
        let mut end = name.len().min(limit);
        while !name.is_char_boundary(end) {
            end -= 1;
        }
        let Ok(name) = std::ffi::CString::new(&name[..end]) else {
            return;
        };
        unsafe {
            #[cfg(target_os = "macos")]
            libc::pthread_setname_np(name.as_ptr());
            #[cfg(not(target_os = "macos"))]
            libc::pthread_setname_np(libc::pthread_self(), name.as_ptr());
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod native {
    pub(super) fn get() -> String {
        String::new()
    }
    pub(super) fn set(_: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoped_name_restores_the_worker_after_nested_roles() {
        spawn("profile-worker", || {
            assert_eq!(std::thread::current().name(), Some("profile-worker"));
            let before = native::get();
            {
                let _outer = Scope::enter("pty-feed");
                assert_eq!(native::get(), "pty-feed");
                {
                    let _inner = Scope::enter("pty-read");
                    assert_eq!(native::get(), "pty-read");
                }
                assert_eq!(native::get(), "pty-feed");
            }
            assert_eq!(native::get(), before);
        })
        .join()
        .unwrap();
    }
}
