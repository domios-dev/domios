#[cfg(target_os = "linux")]
use std::io::Read;
use zeroize::Zeroizing;

#[cfg(target_os = "linux")]
struct PipeGuard(libc::c_int);

#[cfg(target_os = "linux")]
impl Drop for PipeGuard {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.0);
        }
    }
}

pub(super) fn discard() {
    std::env::remove_var("DOMIOS_BOOTSTRAP_CONTROLLER_KEY");
    std::env::remove_var("DOMIOS_BOOTSTRAP_NONCE");
    let fd = std::env::var_os("DOMIOS_CLOUD_BINDING_PIPE");
    std::env::remove_var("DOMIOS_CLOUD_BINDING_PIPE");
    #[cfg(target_os = "linux")]
    if fd.as_deref() == Some(std::ffi::OsStr::new("9")) {
        unsafe {
            libc::close(9);
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = fd;
}

pub(super) fn take() -> Result<Option<(String, Zeroizing<String>)>, String> {
    if std::env::var_os("DOMIOS_BOOTSTRAP_CONTROLLER_KEY").is_some()
        || std::env::var_os("DOMIOS_BOOTSTRAP_NONCE").is_some()
    {
        return Err("Use the cloud bootstrap launcher to pass the creation binding".into());
    }
    let Some(fd) = std::env::var_os("DOMIOS_CLOUD_BINDING_PIPE") else {
        return Ok(None);
    };
    std::env::remove_var("DOMIOS_CLOUD_BINDING_PIPE");
    if fd != "9" {
        return Err("Invalid cloud binding descriptor".into());
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::{FileTypeExt, OpenOptionsExt};
        let _guard = PipeGuard(9);
        let mut pipe = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open("/proc/self/fd/9")
            .map_err(|_| "Missing cloud binding pipe")?;
        if !pipe
            .metadata()
            .map_err(|_| "Invalid cloud binding pipe")?
            .file_type()
            .is_fifo()
        {
            return Err("Invalid cloud binding pipe".into());
        }
        let mut text = Zeroizing::new(String::new());
        (&mut pipe)
            .take(89)
            .read_to_string(&mut text)
            .map_err(|_| "Invalid cloud binding")?;
        if text.len() != 88 || !text.ends_with('\n') {
            return Err("Invalid cloud binding".into());
        }
        let (key, nonce) = text
            .trim_end_matches('\n')
            .split_once('\n')
            .ok_or("Invalid cloud binding")?;
        Ok(Some((key.to_owned(), Zeroizing::new(nonce.to_owned()))))
    }
    #[cfg(not(target_os = "linux"))]
    Err("Timeweb cloud bootstrap requires Linux".into())
}
