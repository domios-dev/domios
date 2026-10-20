use std::fs::{File, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static REVISION: AtomicU64 = AtomicU64::new(0);

pub(super) fn revision() -> u64 {
    REVISION.load(Ordering::Acquire)
}

pub(super) fn changed() {
    REVISION.fetch_add(1, Ordering::Release);
}

pub(super) struct MachineUpdateLock(File);

pub(super) const STATE_FILE: &str = "provider-update-state.json";

pub(super) fn transact<T>(
    change: impl FnOnce(&mut super::state::Coordinator) -> T,
) -> Result<T, String> {
    let dir =
        provider_update_state_dir().ok_or_else(|| "home directory is unavailable".to_string())?;
    transact_in(&dir, change)
}

pub(super) fn transact_in<T>(
    dir: &Path,
    change: impl FnOnce(&mut super::state::Coordinator) -> T,
) -> Result<T, String> {
    let _lock = acquire_machine_update_lock(dir)?;
    let path = dir.join(STATE_FILE);
    let mut state: super::state::Coordinator = match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| error.to_string())?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Default::default(),
        Err(error) => return Err(error.to_string()),
    };
    let before = state.clone();
    let value = change(&mut state);
    if state != before {
        let bytes = serde_json::to_vec(&state).map_err(|error| error.to_string())?;
        codeterm_util::atomic_write::write_atomic(&path, bytes)
            .map_err(|error| error.to_string())?;
        changed();
    }
    Ok(value)
}

pub(super) fn read() -> Result<super::state::Coordinator, String> {
    transact(|state| state.clone())
}

fn acquire_machine_update_lock(dir: &Path) -> Result<MachineUpdateLock, String> {
    let path = dir.join("provider-update.lock");
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    #[cfg(unix)]
    let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) == 0 };
    #[cfg(windows)]
    let locked = lock_windows_file(&file, true);
    if !locked {
        return Err(
            "another Domios instance is already updating a provider on this machine".into(),
        );
    }
    file.set_len(0).map_err(|error| error.to_string())?;
    writeln!(&file, "{}", std::process::id()).map_err(|error| error.to_string())?;
    Ok(MachineUpdateLock(file))
}

pub(super) fn provider_update_state_dir() -> Option<PathBuf> {
    let path = dirs::home_dir()?.join(".codeterm");
    std::fs::create_dir_all(&path).ok()?;
    Some(path)
}

impl Drop for MachineUpdateLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
        #[cfg(windows)]
        lock_windows_file(&self.0, false);
    }
}

#[cfg(windows)]
fn lock_windows_file(file: &File, acquire: bool) -> bool {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;

    #[repr(C)]
    struct Overlapped {
        internal: usize,
        internal_high: usize,
        offset: u32,
        offset_high: u32,
        event: *mut c_void,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LockFileEx(
            handle: *mut c_void,
            flags: u32,
            reserved: u32,
            low: u32,
            high: u32,
            overlapped: *mut Overlapped,
        ) -> i32;
        fn UnlockFileEx(
            handle: *mut c_void,
            reserved: u32,
            low: u32,
            high: u32,
            overlapped: *mut Overlapped,
        ) -> i32;
    }

    let mut overlapped = Overlapped {
        internal: 0,
        internal_high: 0,
        offset: 0,
        offset_high: 0,
        event: std::ptr::null_mut(),
    };
    unsafe {
        if acquire {
            LockFileEx(file.as_raw_handle().cast(), 0x2, 0, 1, 0, &mut overlapped) != 0
        } else {
            UnlockFileEx(file.as_raw_handle().cast(), 0, 1, 0, &mut overlapped) != 0
        }
    }
}
