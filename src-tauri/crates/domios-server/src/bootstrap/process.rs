use super::protocol::ProcessProvenance;
use std::io;

#[cfg(windows)]
#[path = "process_windows.rs"]
mod platform;
#[cfg(unix)]
#[path = "process_unix.rs"]
mod platform;
pub use platform::PinnedProcess;

pub fn own() -> io::Result<PinnedProcess> {
    PinnedProcess::open(std::process::id())
}

pub fn verify_peer(
    peer: &PinnedProcess,
    expected: &std::path::Path,
) -> io::Result<ProcessProvenance> {
    peer.ensure_alive()?;
    let observed = peer.provenance()?;
    if observed.executable != expected.canonicalize()? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Unexpected bootstrap executable",
        ));
    }
    Ok(observed)
}
