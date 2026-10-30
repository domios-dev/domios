use crate::stream_plan::StreamPort;
use fsevent_sys::{self as fs, core_foundation as cf};
use std::ffi::{c_void, CStr, CString, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(crate) struct NativeEvent {
    pub paths: Vec<PathBuf>,
    pub rescan: bool,
    pub topology: bool,
}

type Callback = Arc<dyn Fn(NativeEvent) + Send + Sync>;

#[link(name = "System")]
extern "C" {
    fn dispatch_queue_create(
        label: *const std::ffi::c_char,
        attribute: *const c_void,
    ) -> *mut c_void;
    fn dispatch_release(object: *mut c_void);
    fn dispatch_sync_f(
        queue: *mut c_void,
        context: *mut c_void,
        callback: extern "C" fn(*mut c_void),
    );
}

#[link(name = "CoreServices", kind = "framework")]
extern "C" {
    fn FSEventStreamSetDispatchQueue(stream: fs::FSEventStreamRef, queue: *mut c_void);
}

struct Array(cf::CFArrayRef);

impl Array {
    fn paths(paths: &[PathBuf]) -> Result<Self, String> {
        let array = unsafe {
            cf::CFArrayCreateMutable(
                cf::kCFAllocatorDefault,
                paths.len() as cf::CFIndex,
                &cf::kCFTypeArrayCallBacks,
            )
        };
        if array.is_null() {
            return Err("could not allocate FSEvents path array".into());
        }
        let result = Self(array);
        for path in paths {
            let bytes =
                CString::new(path.as_os_str().as_bytes()).map_err(|error| error.to_string())?;
            let value = unsafe {
                cf::CFStringCreateWithCString(
                    cf::kCFAllocatorDefault,
                    bytes.as_ptr(),
                    cf::kCFStringEncodingUTF8,
                )
            };
            if value.is_null() {
                return Err(format!("FSEvents path is not UTF-8: {}", path.display()));
            }
            unsafe {
                cf::CFArrayAppendValue(array, value);
                cf::CFRelease(value);
            }
        }
        Ok(result)
    }
}

impl Drop for Array {
    fn drop(&mut self) {
        unsafe {
            cf::CFRelease(self.0);
        }
    }
}

struct Context {
    callback: Callback,
}

extern "C" fn release_context(info: *const c_void) {
    if !info.is_null() {
        unsafe {
            Arc::decrement_strong_count(info as *const Context);
        }
    }
}

extern "C" fn retain_context(info: *const c_void) -> *const c_void {
    if !info.is_null() {
        unsafe {
            Arc::increment_strong_count(info as *const Context);
        }
    }
    info
}

extern "C" fn observe(
    _: fs::FSEventStreamRef,
    info: *mut c_void,
    count: usize,
    paths: *mut c_void,
    flags: *const fs::FSEventStreamEventFlags,
    _: *const fs::FSEventStreamEventId,
) {
    if info.is_null() || paths.is_null() || flags.is_null() {
        return;
    }
    let context = unsafe { &*(info as *const Context) };
    let path_array = paths as *const *const std::ffi::c_char;
    let mut event = NativeEvent {
        paths: Vec::new(),
        rescan: count > 8192,
        topology: false,
    };
    for index in 0..count.min(8192) {
        let path = unsafe { *path_array.add(index) };
        let flag = unsafe { *flags.add(index) };
        event.rescan |= flag
            & (fs::kFSEventStreamEventFlagMustScanSubDirs
                | fs::kFSEventStreamEventFlagUserDropped
                | fs::kFSEventStreamEventFlagKernelDropped)
            != 0;
        event.topology |= flag & fs::kFSEventStreamEventFlagRootChanged != 0
            || (flag & fs::kFSEventStreamEventFlagItemIsDir != 0
                && flag
                    & (fs::kFSEventStreamEventFlagItemCreated
                        | fs::kFSEventStreamEventFlagItemRemoved
                        | fs::kFSEventStreamEventFlagItemRenamed)
                    != 0);
        if !path.is_null() {
            event.paths.push(
                Path::new(OsStr::from_bytes(
                    unsafe { CStr::from_ptr(path) }.to_bytes(),
                ))
                .to_path_buf(),
            );
        }
    }
    (context.callback)(event);
}

pub(crate) struct Stream {
    raw: fs::FSEventStreamRef,
    queue: *mut c_void,
    _paths: Array,
    exclusions: Option<Array>,
    started: bool,
}

extern "C" fn barrier(_: *mut c_void) {}

impl Drop for Stream {
    fn drop(&mut self) {
        unsafe {
            if self.started {
                fs::FSEventStreamStop(self.raw);
            }
            fs::FSEventStreamInvalidate(self.raw);
            // Drain callbacks before releasing their context.
            dispatch_sync_f(self.queue, std::ptr::null_mut(), barrier);
            fs::FSEventStreamRelease(self.raw);
            dispatch_release(self.queue);
        }
    }
}

pub(crate) struct NativePort {
    callback: Callback,
}

impl NativePort {
    pub fn new(callback: impl Fn(NativeEvent) + Send + Sync + 'static) -> Self {
        Self {
            callback: Arc::new(callback),
        }
    }
}

impl StreamPort for NativePort {
    type Stream = Stream;

    fn create(&mut self, root: &Path) -> Result<Stream, String> {
        let metadata = root.symlink_metadata().map_err(|error| error.to_string())?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(format!(
                "FSEvents root admission refused: {}",
                root.display()
            ));
        }
        let paths = Array::paths(&[root.to_path_buf()])?;
        let queue = unsafe {
            dispatch_queue_create(c"domios.repository.fsevents".as_ptr(), std::ptr::null())
        };
        if queue.is_null() {
            return Err("could not allocate FSEvents dispatch queue".into());
        }
        let owner = Arc::new(Context {
            callback: self.callback.clone(),
        });
        let context = fs::FSEventStreamContext {
            version: 0,
            info: Arc::as_ptr(&owner) as *mut c_void,
            retain: Some(retain_context),
            release: Some(release_context),
            copy_description: None,
        };
        let raw = unsafe {
            fs::FSEventStreamCreate(
                cf::kCFAllocatorDefault,
                observe,
                &context,
                paths.0,
                fs::kFSEventStreamEventIdSinceNow,
                crate::DEBOUNCE.as_secs_f64(),
                fs::kFSEventStreamCreateFlagFileEvents
                    | fs::kFSEventStreamCreateFlagWatchRoot
                    | fs::kFSEventStreamCreateFlagNoDefer,
            )
        };
        if raw.is_null() {
            unsafe {
                dispatch_release(queue);
            }
            return Err(format!("FSEventStreamCreate refused {}", root.display()));
        }
        unsafe {
            FSEventStreamSetDispatchQueue(raw, queue);
        }
        Ok(Stream {
            raw,
            queue,
            _paths: paths,
            exclusions: None,
            started: false,
        })
    }

    fn exclude(&mut self, stream: &mut Stream, paths: &[PathBuf]) -> Result<(), String> {
        let exclusions = Array::paths(paths)?;
        if unsafe { fs::FSEventStreamSetExclusionPaths(stream.raw, exclusions.0) } == 0 {
            return Err("FSEventStreamSetExclusionPaths refused the exclusion list".into());
        }
        stream.exclusions = Some(exclusions);
        Ok(())
    }

    fn start(&mut self, stream: &mut Stream) -> Result<(), String> {
        if unsafe { fs::FSEventStreamStart(stream.raw) } == 0 {
            return Err("FSEventStreamStart failed".into());
        }
        stream.started = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
