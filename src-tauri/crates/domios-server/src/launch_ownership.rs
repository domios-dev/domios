use codeterm_process_identity::PinnedProcess;
use codeterm_types::server_bootstrap::ProcessProvenance;
use std::io;
use tokio::sync::oneshot;

pub(crate) fn watch_owner(
    expected: Option<&ProcessProvenance>,
) -> io::Result<Option<oneshot::Receiver<io::Result<()>>>> {
    let Some(expected) = expected else {
        return Ok(None);
    };
    let owner = PinnedProcess::open(expected.pid)?;
    if owner.provenance()? != *expected {
        return Err(io::Error::other("Development launch owner identity changed"));
    }
    let (sender, receiver) = oneshot::channel();
    std::thread::Builder::new()
        .name("dev-launch-owner".into())
        .spawn(move || {
            let _ = sender.send(owner.wait_for_exit());
        })?;
    Ok(Some(receiver))
}

pub(crate) fn retire_unattached(owner_gone: bool, attached_once: bool) -> bool {
    owner_gone && !attached_once
}

#[cfg(test)]
mod tests;
