use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Weak,
    },
    time::Duration,
};

use async_trait::async_trait;
use codeterm_plugin::transcribe::{
    DictationAdmission, DictationSession, TranscribeRegistry, Transcriber,
};
use codeterm_types::dictation::{
    DictationSegment, DictationStart, DictationStartResult, DictationUpdate, MAX_PCM_BYTES,
    MAX_UPDATE_BYTES,
};
use parking_lot::Mutex;
use serde::Serialize;
use tokio::{
    sync::{mpsc, oneshot, watch},
    time::{sleep_until, Instant},
};

use super::ServiceContext;

pub mod live_capture;
mod rotation;

use rotation::{cancelled, RotatingEngine, RotationOutput};

const IDLE: Duration = Duration::from_secs(45);
// A stalled composer can still hold three minutes of audio in its worklet pipe.
const COMPOSER_IDLE: Duration = Duration::from_secs(240);
const EXCHANGE: Duration = Duration::from_secs(18);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationFrameResult {
    #[serde(flatten)]
    pub update: DictationUpdate,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub prior_committed: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
}

type ClientResult = Result<DictationFrameResult, String>;
type Slot = Arc<Mutex<Option<Recording>>>;

#[derive(Default)]
pub struct DictationSessions {
    slot: Slot,
    microphone: Arc<AtomicBool>,
}

struct MicrophoneLease(Arc<AtomicBool>);

impl Drop for MicrophoneLease {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

struct Recording {
    lease: String,
    client: String,
    frames: mpsc::Sender<Frame>,
    cancel: watch::Sender<bool>,
}

struct Frame {
    sequence: u32,
    pcm: Vec<u8>,
    finish: bool,
    response: oneshot::Sender<ClientResult>,
}

struct Claim {
    slot: Slot,
    session: String,
    cancellation: watch::Receiver<bool>,
    frames: mpsc::Receiver<Frame>,
    _microphone: MicrophoneLease,
}

impl Drop for Claim {
    fn drop(&mut self) {
        self.slot.lock().take();
    }
}

#[derive(Clone, PartialEq, Eq)]
struct TabIdentity {
    pty: Option<String>,
    session: Option<String>,
    nonce: Option<String>,
    created_at: u64,
}

impl TabIdentity {
    fn read(svc: &ServiceContext, tab_id: &str) -> Option<Self> {
        let mux = svc.mux.read();
        let tab = mux.get_tab(tab_id)?;
        Some(Self {
            pty: tab.pty_id.clone(),
            session: tab.agent_session_id.clone(),
            nonce: tab.session_binding_nonce.clone(),
            created_at: tab.created_at_ms,
        })
    }
}

impl DictationSessions {
    fn claim_microphone(&self) -> Result<MicrophoneLease, String> {
        self.microphone
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .map(|_| MicrophoneLease(self.microphone.clone()))
            .map_err(|_| "Another microphone recording is already active.".into())
    }

    fn claim(&self, client: &str, session: &str) -> Result<Claim, String> {
        let microphone = self.claim_microphone()?;
        let mut slot = self.slot.lock();
        let (cancel, cancellation) = watch::channel(false);
        let (tx, frames) = mpsc::channel(1);
        *slot = Some(Recording {
            lease: session.into(),
            client: client.into(),
            frames: tx,
            cancel,
        });
        Ok(Claim {
            slot: self.slot.clone(),
            session: session.into(),
            cancellation,
            frames,
            _microphone: microphone,
        })
    }

    pub fn cancel(&self, client: &str, session: &str) {
        if let Some(recording) = self
            .slot
            .lock()
            .as_ref()
            .filter(|recording| recording.matches(client, session))
        {
            recording.cancel.send_replace(true);
        }
    }

    pub async fn frame(
        &self,
        client: &str,
        session: &str,
        sequence: u32,
        pcm: Vec<u8>,
        finish: bool,
    ) -> ClientResult {
        if sequence == 0
            || pcm.is_empty() != finish
            || pcm.len() > MAX_PCM_BYTES
            || pcm.len() % 2 != 0
        {
            return Err("Invalid dictation frame.".into());
        }
        let (response, result) = oneshot::channel();
        {
            let slot = self.slot.lock();
            let recording = slot
                .as_ref()
                .filter(|recording| recording.matches(client, session))
                .ok_or("The dictation session has ended.")?;
            if *recording.cancel.borrow() {
                return Err("The dictation session was cancelled.".into());
            }
            recording
                .frames
                .try_send(Frame {
                    sequence,
                    pcm,
                    finish,
                    response,
                })
                .map_err(|_| "The dictation worker cannot accept another frame.")?;
        }
        result
            .await
            .map_err(|_| "The dictation session has ended.")?
    }
}

impl Recording {
    fn matches(&self, client: &str, session: &str) -> bool {
        self.client == client && self.lease == session
    }
}

impl ServiceContext {
    pub async fn start_dictation(
        self: &Arc<Self>,
        client: &str,
        request: DictationStart,
    ) -> DictationStartResult {
        let unavailable = |reason: String| DictationStartResult::Unavailable { reason };
        let Some(tab) = TabIdentity::read(self, &request.tab_id) else {
            return unavailable("The dictation composer is no longer open.".into());
        };
        let mut claim = match self
            .ext::<DictationSessions>()
            .claim(client, &request.session)
        {
            Ok(claim) => claim,
            Err(error) => return unavailable(error),
        };
        let registry = self.ext::<Arc<TranscribeRegistry>>().clone();
        let preferred = self.configuration().integrations.voice_transcribe.active;
        let Some((backend, registration)) = select_backend(&registry, &preferred) else {
            return DictationStartResult::Setup {
                reason: "A live dictation plugin is needed on this device.".into(),
            };
        };
        let authority = Authority {
            svc: Arc::downgrade(self),
            registry,
            tab,
            tab_id: request.tab_id,
            plugin: backend.id().to_owned(),
            registration,
        };
        let (reply, started) = oneshot::channel();
        tokio::spawn(async move {
            let admission = backend.start_dictation().await;
            let engine = match admission {
                DictationAdmission::Ready(engine) => engine,
                DictationAdmission::Setup(reason) => {
                    let _ = reply.send(DictationStartResult::Setup { reason });
                    return;
                }
                DictationAdmission::Unavailable(reason) => {
                    let _ = reply.send(unavailable(reason));
                    return;
                }
            };
            if *claim.cancellation.borrow()
                || !authority.current()
                || reply.send(DictationStartResult::Ready).is_err()
            {
                engine.close().await;
                return;
            }
            let mut engine = RotatingEngine::new(backend, engine);
            let terminal = run(&mut claim, &authority, &mut engine).await;
            engine.close().await;
            let current = !*claim.cancellation.borrow() && authority.current();
            drop(claim);
            if let Some((reply, result)) = terminal {
                let _ = reply.send(if current {
                    result
                } else {
                    Err("The dictation session ended.".into())
                });
            }
        });
        started
            .await
            .unwrap_or_else(|_| unavailable("The dictation worker did not start.".into()))
    }
}

fn select_backend(
    registry: &TranscribeRegistry,
    preferred: &str,
) -> Option<(Arc<dyn Transcriber>, String)> {
    registry
        .registered(preferred)
        .filter(|(plugin, _)| plugin.supports_dictation())
        .or_else(|| {
            let mut ids = registry.ids();
            ids.sort_unstable();
            ids.into_iter()
                .filter_map(|id| registry.registered(&id))
                .find(|(plugin, _)| plugin.supports_dictation())
        })
}

struct Authority {
    svc: Weak<ServiceContext>,
    registry: Arc<TranscribeRegistry>,
    tab: TabIdentity,
    tab_id: String,
    plugin: String,
    registration: String,
}

impl Authority {
    fn current(&self) -> bool {
        self.registry.registration_id(&self.plugin).as_ref() == Some(&self.registration)
            && self.svc.upgrade().is_some_and(|svc| {
                TabIdentity::read(&svc, &self.tab_id).as_ref() == Some(&self.tab)
            })
    }
}

struct ComposerOutput<'a> {
    authority: &'a Authority,
    cancel: watch::Receiver<bool>,
    committed: Vec<String>,
}

#[async_trait]
impl RotationOutput for ComposerOutput<'_> {
    fn check(&self) -> Result<(), String> {
        if *self.cancel.borrow() || !self.authority.current() {
            return Err("The dictation composer or plugin changed.".into());
        }
        Ok(())
    }

    fn cancellation(&self) -> &watch::Receiver<bool> {
        &self.cancel
    }

    async fn accept(
        &mut self,
        segment: DictationSegment,
        finished: bool,
    ) -> Result<String, String> {
        if !segment.committed.is_empty() {
            self.committed.push(segment.committed);
        }
        if finished && !segment.provisional.is_empty() {
            self.committed.push(segment.provisional);
            return Ok(String::new());
        }
        Ok(segment.provisional)
    }
}

async fn run(
    claim: &mut Claim,
    authority: &Authority,
    engine: &mut RotatingEngine,
) -> Option<(oneshot::Sender<ClientResult>, ClientResult)> {
    let mut next = 1u32;
    let mut idle = Instant::now() + COMPOSER_IDLE;
    loop {
        let frame = tokio::select! {
            biased;
            _ = cancelled(&mut claim.cancellation) => return None,
            _ = sleep_until(idle) => return None,
            frame = claim.frames.recv() => frame?,
        };
        if frame.sequence != next || next == u32::MAX {
            let _ = frame
                .response
                .send(Err("The dictation frame is out of order.".into()));
            continue;
        }
        let mut output = ComposerOutput {
            authority,
            cancel: claim.cancellation.clone(),
            committed: Vec::new(),
        };
        let result = if frame.finish {
            engine.close_standby().await;
            engine
                .finish_engine(&mut output)
                .await
                .map(|()| String::new())
        } else {
            engine.audio(&mut output, frame.pcm).await
        };
        let stopped = result.is_err();
        let result = match result {
            Err(reason) if output.committed.is_empty() => Err(reason),
            result => {
                let (provisional, stop_reason) = match result {
                    Ok(provisional) => (provisional, None),
                    Err(reason) => (String::new(), Some(reason)),
                };
                // On rotation, each engine reply retains its own transcript bound.
                let committed = if provisional.len()
                    + output.committed.last().map_or(0, String::len)
                    <= MAX_UPDATE_BYTES
                {
                    output.committed.pop().unwrap_or_default()
                } else {
                    String::new()
                };
                Ok(DictationFrameResult {
                    update: DictationUpdate {
                        session: claim.session.clone(),
                        sequence: next,
                        committed,
                        provisional,
                        is_final: frame.finish,
                    },
                    prior_committed: output.committed,
                    stop_reason,
                })
            }
        };
        if stopped || frame.finish {
            return Some((frame.response, result));
        }
        if frame.response.send(result).is_err() {
            return None;
        }
        next += 1;
        idle = Instant::now() + COMPOSER_IDLE;
    }
}

#[cfg(test)]
mod tests;
