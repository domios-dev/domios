use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Weak,
    },
    time::Duration,
};

use codeterm_plugin::transcribe::{
    DictationAdmission, DictationSession, TranscribeRegistry, Transcriber,
};
use codeterm_types::dictation::{
    DictationStart, DictationStartResult, DictationUpdate, MAX_AUDIO_BYTES, MAX_PCM_BYTES,
    MAX_UPDATE_BYTES,
};
use parking_lot::Mutex;
use tokio::{
    sync::{mpsc, oneshot, watch},
    time::{sleep_until, Instant},
};

use super::ServiceContext;

pub mod live_capture;

const IDLE: Duration = Duration::from_secs(45);
const LIFETIME: Duration = Duration::from_secs(345);
const EXCHANGE: Duration = Duration::from_secs(18);
const MAX_FRAMES: u32 = (MAX_AUDIO_BYTES / MAX_PCM_BYTES) as u32 + 2;

type ClientResult = Result<DictationUpdate, String>;
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
            || sequence > MAX_FRAMES
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
            drop(backend);
            let mut engine = match admission {
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
            let terminal = run(&mut claim, &authority, engine.as_mut()).await;
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

async fn run(
    claim: &mut Claim,
    authority: &Authority,
    engine: &mut dyn DictationSession,
) -> Option<(oneshot::Sender<ClientResult>, ClientResult)> {
    let expires = Instant::now() + LIFETIME;
    let mut idle = Instant::now() + IDLE;
    let mut next = 1;
    let mut bytes = 0;
    loop {
        let frame = tokio::select! {
            biased;
            _ = claim.cancellation.changed() => return None,
            _ = sleep_until(expires.min(idle)) => return None,
            frame = claim.frames.recv() => frame?,
        };
        if frame.sequence != next {
            let _ = frame
                .response
                .send(Err("The dictation frame is out of order.".into()));
            continue;
        }
        if !authority.current() {
            let _ = frame
                .response
                .send(Err("The dictation composer or plugin changed.".into()));
            return None;
        }
        bytes += frame.pcm.len();
        if bytes > MAX_AUDIO_BYTES {
            let _ = frame
                .response
                .send(Err("The dictation audio limit was reached.".into()));
            return None;
        }
        let segment = tokio::select! {
            biased;
            _ = claim.cancellation.changed() => return None,
            _ = sleep_until(expires.min(Instant::now() + EXCHANGE)) => {
                let _ = frame.response.send(Err("The dictation worker did not answer in time.".into()));
                return None;
            }
            segment = engine.update(frame.pcm, frame.finish) => segment,
        };
        let result = segment.and_then(|segment| {
            if *claim.cancellation.borrow() || !authority.current() {
                return Err("The dictation composer or plugin changed.".into());
            }
            if segment.committed.len() + segment.provisional.len() > MAX_UPDATE_BYTES {
                return Err("The dictation worker exceeded its transcript limit.".into());
            }
            Ok(DictationUpdate {
                session: claim.session.clone(),
                sequence: next,
                committed: segment.committed,
                provisional: segment.provisional,
                is_final: frame.finish,
            })
        });
        if result.is_err() || frame.finish {
            return Some((frame.response, result));
        }
        if frame.response.send(result).is_err() {
            return None;
        }
        next += 1;
        idle = Instant::now() + IDLE;
    }
}

#[cfg(test)]
mod tests;
