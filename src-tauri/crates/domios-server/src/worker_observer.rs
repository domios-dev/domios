use codeterm_service_core::mem::supervisor::observation::WorkerSpawn;
use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    sync::mpsc::{sync_channel, SyncSender},
};

pub fn start(data: &Path, selected: PathBuf) -> io::Result<SyncSender<WorkerSpawn>> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut sink = options.open(data.join("process-worker.jsonl"))?;
    let (sender, receiver) = sync_channel::<WorkerSpawn>(4);
    std::thread::Builder::new().name("worker-probe-observer".into()).spawn(move || {
        for event in receiver.into_iter().take(4) {
            let pid = event.pid;
            let observation = (|| {
                if event.executable != selected { return Err(io::Error::other("Worker selection changed")); }
                let pin = crate::bootstrap::process::PinnedProcess::from_child_witness(pid, event.witness?)?;
                let provenance = pin.provenance()?;
                if provenance.executable != selected { return Err(io::Error::other("Spawned worker executable differs")); }
                Ok(provenance)
            })();
            let row = match observation {
                Ok(provenance) => serde_json::json!({ "status": "verified", "process": provenance, "selected": selected, "resource_sha256": env!("SERVER_CLI_SHA256") }),
                Err(_) => serde_json::json!({ "status": "unavailable", "pid": pid }),
            };
            if writeln!(sink, "{row}").and_then(|_| sink.sync_data()).is_err() { break; }
        }
    })?;
    Ok(sender)
}
