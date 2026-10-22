use std::fs::{File, Metadata, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "macos")]
mod macos;

const WRITING: &str = "The file is still being written; drop it again when it is ready";

struct Source {
    file: File,
    before: Metadata,
}

impl Source {
    fn open(path: &Path, limit: u64) -> Result<Self, String> {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK);
        }
        let file = options.open(path).map_err(|error| {
            if cfg!(windows) && error.raw_os_error() == Some(32) {
                format!("{WRITING} ({error})")
            } else {
                format!("Cannot open attachment: {error}")
            }
        })?;
        let before = file.metadata().map_err(|e| e.to_string())?;
        if !before.is_file() {
            return Err("Attachment source is not a regular file".into());
        }
        if before.len() == 0 {
            return Err("Empty file".into());
        }
        if before.len() > limit {
            return Err(format!("Attachment exceeds {limit} bytes"));
        }
        ensure_no_writer(path, &before)?;
        Ok(Self { file, before })
    }

    fn copy_to(&mut self, path: &Path, output: &mut impl Write) -> Result<u64, String> {
        let size = self.before.len();
        let copied = std::io::copy(&mut (&mut self.file).take(size), output)
            .map_err(|e| format!("Copy attachment: {e}"))?;
        ensure_no_writer(path, &self.before)?;
        let after = self.file.metadata().map_err(|e| e.to_string())?;
        let named = std::fs::metadata(path).map_err(|_| WRITING.to_string())?;
        if copied != size || !unchanged(&self.before, &after) || !unchanged(&self.before, &named) {
            return Err(WRITING.into());
        }
        Ok(copied)
    }
}

fn ensure_no_writer(path: &Path, metadata: &Metadata) -> Result<(), String> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        platform::ensure_no_writer(path, metadata)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (path, metadata);
        Ok(())
    }
}

fn unchanged(before: &Metadata, after: &Metadata) -> bool {
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != after.dev()
            || before.ino() != after.ino()
            || before.ctime() != after.ctime()
            || before.ctime_nsec() != after.ctime_nsec()
        {
            return false;
        }
    }
    true
}

pub fn copy_complete_file(source: &Path, target: &Path, limit: u64) -> Result<u64, String> {
    let mut source_file = Source::open(source, limit)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|e| format!("Create attachment: {e}"))?;
    let result = source_file.copy_to(source, &mut output).and_then(|size| {
        output.flush().map_err(|e| e.to_string())?;
        Ok(size)
    });
    drop(output);
    if result.is_err() {
        let _ = std::fs::remove_file(target);
    }
    result
}

pub fn read_complete_file(source: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let mut source_file = Source::open(source, limit)?;
    let mut bytes = Vec::new();
    source_file.copy_to(source, &mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests;
