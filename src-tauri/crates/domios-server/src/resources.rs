use codeterm_service_core::ports::host::Shell;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Seek},
    path::{Path, PathBuf},
};

pub struct WorkerCli {
    _file: File,
    _alias: File,
    _lease: tempfile::TempDir,
    pub path: PathBuf,
}

pub fn pin_worker_cli(shell: &dyn Shell, expected: &str) -> io::Result<WorkerCli> {
    if expected.len() != 64 {
        return Err(io::Error::other(
            "Server has no verified worker CLI artifact identity",
        ));
    }
    let path = codeterm_service_core::startup::resolve_bundled_cli_src(shell)
        .ok_or_else(|| io::Error::other("Server worker CLI resource unavailable"))?
        .canonicalize()?;
    let worker = verify_worker_cli(&path, expected)?;
    std::env::set_var("CODETERM_CLI_PATH", &worker.path);
    Ok(worker)
}

fn verify_worker_cli(path: &Path, expected: &str) -> io::Result<WorkerCli> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ);
    }
    let mut file = options.open(path)?;
    verify_digest(&mut file, expected)?;
    let lease = tempfile::Builder::new()
        .prefix("domios-worker-")
        .tempdir()?;
    let destination = lease.path().join(if cfg!(windows) {
        "codeterm.exe"
    } else {
        "codeterm"
    });
    let mut copy = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)?;
    file.rewind()?;
    let started = std::time::Instant::now();
    let bytes = std::io::copy(&mut file, &mut copy)?;
    copy.sync_all()?;
    log::debug!(
        "[boot-operation] worker CLI pinned copy bytes={} elapsed_ms={}",
        bytes,
        started.elapsed().as_millis()
    );
    drop(copy);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&destination, std::fs::Permissions::from_mode(0o500))?;
    }
    let mut pinned = options.open(&destination)?;
    verify_digest(&mut pinned, expected)?;
    let alias = lease
        .path()
        .join(format!("domios{}", std::env::consts::EXE_SUFFIX));
    let started = std::time::Instant::now();
    let bytes = std::fs::copy(&destination, &alias)?;
    log::debug!(
        "[boot-operation] worker CLI alias copy bytes={} elapsed_ms={}",
        bytes,
        started.elapsed().as_millis()
    );
    let mut pinned_alias = options.open(&alias)?;
    verify_digest(&mut pinned_alias, expected)?;
    Ok(WorkerCli {
        _file: pinned,
        _alias: pinned_alias,
        _lease: lease,
        path: destination.canonicalize()?,
    })
}

fn verify_digest(file: &mut File, expected: &str) -> io::Result<()> {
    let started = std::time::Instant::now();
    let mut hash = Sha256::new();
    let bytes = std::io::copy(file, &mut hash)?;
    let digest = format!("{:x}", hash.finalize());
    log::debug!(
        "[boot-operation] worker CLI integrity hash bytes={} elapsed_ms={}",
        bytes,
        started.elapsed().as_millis()
    );
    if digest != expected {
        return Err(io::Error::other(
            "Worker CLI does not match the server's built CLI artifact",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
