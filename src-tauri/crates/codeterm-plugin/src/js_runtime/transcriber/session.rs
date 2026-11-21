use std::{
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    sync::{Arc, Weak},
};

use async_trait::async_trait;
use codeterm_types::dictation::{DictationSegment, MAX_PCM_BYTES, MAX_UPDATE_BYTES};
use parking_lot::Mutex;
use serde::Deserialize;
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

use crate::transcribe::{DictationAdmission, DictationSession};

use super::super::{
    host_exec::child::Target,
    vm::{call_json_async_until, PluginVm},
};

#[derive(Deserialize)]
#[serde(tag = "state", rename_all = "camelCase", deny_unknown_fields)]
enum Launch {
    Ready {
        bin: String,
        script: PathBuf,
        args: Vec<String>,
    },
    Setup {
        reason: String,
    },
    Unavailable {
        reason: String,
    },
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Reply {
    Segment(DictationSegment),
    Error { error: String },
}

struct Request {
    pcm: Vec<u8>,
    reply: oneshot::Sender<Result<DictationSegment, String>>,
}

struct Session {
    tx: mpsc::Sender<Request>,
    target: Arc<Mutex<Option<Target>>>,
    worker: Option<JoinHandle<()>>,
    vm: Weak<PluginVm>,
    generation: u64,
}

impl Session {
    fn current(&self) -> bool {
        self.vm.upgrade().is_some_and(|vm| vm.dictation_generation() == self.generation)
    }

    fn terminate(&self) {
        if let Some(target) = self.target.lock().take() {
            target.terminate();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[async_trait]
impl DictationSession for Session {
    async fn update(&mut self, pcm: Vec<u8>, finish: bool) -> Result<DictationSegment, String> {
        if !self.current() {
            return Err("The dictation model or plugin changed.".into());
        }
        if pcm.is_empty() != finish || pcm.len() > MAX_PCM_BYTES || pcm.len() % 2 != 0 {
            return Err("Invalid dictation audio frame.".into());
        }
        let (reply, result) = oneshot::channel();
        self.tx.try_send(Request { pcm, reply }).map_err(|_| "The dictation worker is unavailable.")?;
        let segment = result.await.map_err(|_| "The dictation worker stopped.")??;
        if !self.current() {
            return Err("The dictation model or plugin changed.".into());
        }
        Ok(segment)
    }

    async fn close(mut self: Box<Self>) {
        self.terminate();
        let (closed, _) = mpsc::channel(1);
        self.tx = closed;
        if let Some(worker) = self.worker.take() {
            let _ = worker.await;
        }
    }
}

pub(super) async fn start(vm: Arc<PluginVm>) -> DictationAdmission {
    let generation = vm.dictation_generation();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(12);
    let (bin, script, args) = match call_json_async_until::<Launch>(
        vm.clone(),
        "dictationLaunch",
        Vec::new(),
        (Default::default(), deadline),
    ).await {
        Ok(Launch::Ready { bin, script, args }) => (bin, script, args),
        Ok(Launch::Setup { reason }) => return DictationAdmission::Setup(bounded_reason(reason)),
        Ok(Launch::Unavailable { reason }) => return DictationAdmission::Unavailable(bounded_reason(reason)),
        Err(err) => return DictationAdmission::Unavailable(format!("Cannot prepare the dictation plugin: {err}")),
    };
    if generation != vm.dictation_generation() || bin.is_empty() || bin.len() > 4096
        || args.len() > 12 || args.iter().any(|arg| arg.len() > 4096) {
        return DictationAdmission::Unavailable("The dictation launch configuration changed or is invalid.".into());
    }
    let (tx, mut rx) = mpsc::channel::<Request>(1);
    let (started, ready) = oneshot::channel::<Result<(), String>>();
    let target = Arc::new(Mutex::new(None));
    let worker_target = target.clone();
    let vm_ref = Arc::downgrade(&vm);
    let worker = tokio::task::spawn_blocking(move || {
        let mut child = match vm.dictation_child(&bin, &script, &args) {
            Ok(child) => child,
            Err(error) => { let _ = started.send(Err(error)); return; }
        };
        drop(vm);
        let (Some(mut input), Some(output)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = started.send(Err("The dictation worker's pipes are unavailable.".into()));
            return;
        };
        let mut output = BufReader::new(output);
        *worker_target.lock() = Some(child.target());
        if started.send(Ok(())).is_ok() {
            while let Some(request) = rx.blocking_recv() {
                let finished = request.pcm.is_empty();
                let result = exchange(&mut input, &mut output, &request.pcm);
                let failed = result.is_err();
                if request.reply.send(result).is_err() || failed || finished {
                    break;
                }
            }
        }
        if let Some(target) = worker_target.lock().take() {
            target.terminate();
        }
    });
    let session = Session { tx, target, worker: Some(worker), vm: vm_ref, generation };
    match ready.await {
        Ok(Ok(())) => DictationAdmission::Ready(Box::new(session)),
        Ok(Err(error)) => DictationAdmission::Unavailable(bounded_reason(error)),
        Err(_) => DictationAdmission::Unavailable("The dictation worker did not start.".into()),
    }
}

fn bounded_reason(reason: String) -> String {
    reason.chars().take(512).collect()
}

pub(super) fn exchange(
    input: &mut impl Write,
    output: &mut impl BufRead,
    pcm: &[u8],
) -> Result<DictationSegment, String> {
    input.write_all(&(pcm.len() as u32).to_le_bytes())
        .and_then(|_| input.write_all(pcm))
        .and_then(|_| input.flush())
        .map_err(|_| "Could not write microphone audio to the dictation worker.")?;
    let mut line = Vec::new();
    Read::take(output, MAX_UPDATE_BYTES as u64)
        .read_until(b'\n', &mut line)
        .map_err(|_| "Could not read from the dictation worker.")?;
    if !line.ends_with(b"\n") {
        return Err("The dictation worker closed or exceeded its response limit.".into());
    }
    match serde_json::from_slice(&line).map_err(|_| "The dictation worker sent an invalid response.")? {
        Reply::Segment(segment) => Ok(segment),
        Reply::Error { error } => Err(bounded_reason(error)),
    }
}
