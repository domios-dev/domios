use std::path::Path;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;

struct Probe {
    id: uuid::Uuid,
    cancel: tokio::sync::oneshot::Sender<()>,
    finished: std::sync::mpsc::Receiver<()>,
}

static PROBES: Mutex<Vec<Probe>> = Mutex::new(Vec::new());
static EPOCH: AtomicU64 = AtomicU64::new(0);

pub(crate) fn epoch() -> u64 {
    EPOCH.load(Ordering::SeqCst)
}

pub(crate) fn stop() -> usize {
    let probes = {
        let mut probes = PROBES.lock().unwrap_or_else(|p| p.into_inner());
        EPOCH.fetch_add(1, Ordering::SeqCst);
        std::mem::take(&mut *probes)
    };
    let mut completions = Vec::new();
    for probe in probes {
        let _ = probe.cancel.send(());
        completions.push(probe.finished);
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    completions
        .into_iter()
        .filter(|done| {
            done.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .is_ok()
        })
        .count()
}

pub(crate) fn capture(path: &Path, deadline: Instant, admitted_epoch: u64) -> Option<Vec<u8>> {
    capture_observed(path, deadline, admitted_epoch, |_| {})
}

pub(crate) fn capture_observed(
    path: &Path,
    deadline: Instant,
    admitted_epoch: u64,
    on_spawn: impl FnOnce(u32),
) -> Option<Vec<u8>> {
    if Instant::now() >= deadline {
        return None;
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    runtime.block_on(async {
        let mut command = tokio::process::Command::new(path);
        command.arg("--version").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).kill_on_drop(true);
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(command.as_std_mut(), 0);
        #[cfg(windows)]
        codeterm_util::process_util::no_window_tokio_suspended(&mut command);
        let id = uuid::Uuid::new_v4();
        let (cancel, cancelled) = tokio::sync::oneshot::channel();
        let (finished, completion) = std::sync::mpsc::channel();
        let mut child = {
            let mut probes = PROBES.lock().unwrap_or_else(|p| p.into_inner());
            if epoch() != admitted_epoch || Instant::now() >= deadline { return None; }
            let child = command.spawn().ok()?;
            probes.push(Probe { id, cancel, finished: completion });
            child
        };
        #[cfg(windows)]
        let job = match codeterm_util::windows_job::Job::attach(&child) {
            Ok(job) => Some(job),
            Err(_) => None,
        };
        #[cfg(windows)]
        let owned = job.is_some();
        #[cfg(not(windows))]
        let owned = true;
        #[cfg(unix)]
        let pid = child.id();
        if let Some(pid) = child.id() { on_spawn(pid); }
        let mut reaped = false;
        let result = if owned {
            let produce = async {
                let mut bytes = Vec::new();
                child.stdout.take()?.take(1025).read_to_end(&mut bytes).await.ok()?;
                if bytes.len() > 1024 { return None; }
                let status = child.wait().await.ok()?;
                reaped = true;
                status.success().then_some(bytes)
            };
            tokio::select! {
                result = tokio::time::timeout(deadline.saturating_duration_since(Instant::now()), produce) => result.ok().flatten(),
                _ = cancelled => None,
            }
        } else { None };
        if !reaped {
            #[cfg(unix)]
            if let Some(pid) = pid {
                // The leader remains unreaped until capture completes, retaining this process group identity.
                unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGKILL); }
            }
            #[cfg(windows)]
            if let Some(job) = &job { job.terminate(); }
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
        #[cfg(windows)]
        drop(job);
        PROBES.lock().unwrap_or_else(|p| p.into_inner()).retain(|probe| probe.id != id);
        let _ = finished.send(());
        result
    })
}

#[cfg(test)]
mod tests;
