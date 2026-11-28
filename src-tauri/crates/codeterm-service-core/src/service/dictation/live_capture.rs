use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use codeterm_types::dictation::DictationSegment;
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{mpsc, oneshot, watch},
    time::timeout,
};
use uuid::Uuid;

use super::{
    rotation::{cancelled, open_engine, Engine, RotatingEngine, RotationOutput},
    select_backend, Authority, DictationAdmission, DictationSessions, MicrophoneLease,
    ServiceContext, TabIdentity, TranscribeRegistry, Transcriber, IDLE, MAX_PCM_BYTES,
};

type ResultUpdate = Result<LiveCaptureUpdate, String>;
type Slot = Arc<parking_lot::Mutex<Option<Recording>>>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveCaptureIdentity {
    pub lease: Uuid,
    pub client: Uuid,
    pub tab_id: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PauseReason {
    Manual,
    FocusLost,
    Hidden,
    TabChanged,
    MicrophoneLost,
    Inactive,
    Failed,
}

#[derive(Debug, Eq, PartialEq)]
pub enum LiveCaptureEvent {
    Speech { text: String },
    Paused { reason: PauseReason },
    Resumed,
}

#[async_trait]
pub trait LiveCaptureSink: Send + Sync {
    async fn prepare(&self, _owner: &str) -> Result<(), String> {
        Ok(())
    }

    async fn append(
        &self,
        capture: &LiveCaptureIdentity,
        event: LiveCaptureEvent,
    ) -> Result<(), String>;
}

pub struct LiveCaptureOutput(pub Arc<dyn LiveCaptureSink>);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LiveCaptureStart {
    pub tab_id: String,
    #[serde(default)]
    pub resume: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum LiveCaptureAdmission {
    Ready { lease: Uuid },
    Setup { reason: String },
    Unavailable { reason: String },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveCaptureUpdate {
    pub lease: Uuid,
    pub sequence: u32,
    pub provisional: String,
    pub paused: bool,
}

#[derive(Default)]
pub struct LiveCaptureSessions(Slot);

struct Recording {
    identity: LiveCaptureIdentity,
    commands: mpsc::Sender<Request>,
    cancel: watch::Sender<bool>,
}

enum Command {
    Audio(Vec<u8>),
    Pause(PauseReason),
}

struct Request {
    sequence: u32,
    command: Command,
    reply: oneshot::Sender<ResultUpdate>,
}

struct Claim {
    slot: Slot,
    identity: LiveCaptureIdentity,
    commands: mpsc::Receiver<Request>,
    cancel: watch::Receiver<bool>,
    _microphone: MicrophoneLease,
}

impl Drop for Claim {
    fn drop(&mut self) {
        self.slot.lock().take();
    }
}

impl LiveCaptureSessions {
    pub fn active_for_owner(&self, owner: &str) -> bool {
        self.0.lock().as_ref().is_some_and(|recording| {
            recording.identity.tab_id == owner && !*recording.cancel.borrow()
        })
    }

    fn claim(&self, identity: LiveCaptureIdentity, microphone: MicrophoneLease) -> Claim {
        let (sender, commands) = mpsc::channel(1);
        let (cancel, cancelled) = watch::channel(false);
        *self.0.lock() = Some(Recording {
            identity: identity.clone(),
            commands: sender,
            cancel,
        });
        Claim {
            slot: self.0.clone(),
            identity,
            commands,
            cancel: cancelled,
            _microphone: microphone,
        }
    }

    pub fn cancel(&self, client: Uuid, lease: Uuid) {
        if let Some(recording) = self
            .0
            .lock()
            .as_ref()
            .filter(|r| r.identity.client == client && r.identity.lease == lease)
        {
            recording.cancel.send_replace(true);
        }
    }

    pub async fn frame(
        &self,
        client: Uuid,
        lease: Uuid,
        sequence: u32,
        pcm: Vec<u8>,
    ) -> ResultUpdate {
        if pcm.is_empty() || pcm.len() > MAX_PCM_BYTES || pcm.len() % 2 != 0 {
            return Err("Invalid live capture audio frame.".into());
        }
        self.submit(client, lease, sequence, None, Command::Audio(pcm))
            .await
    }

    pub async fn pause(
        &self,
        client: Uuid,
        lease: Uuid,
        sequence: u32,
        reason: PauseReason,
    ) -> ResultUpdate {
        self.submit(client, lease, sequence, None, Command::Pause(reason))
            .await
    }

    pub async fn pause_owner(
        &self,
        owner: &str,
        client: Uuid,
        lease: Uuid,
        sequence: u32,
    ) -> ResultUpdate {
        self.submit(
            client,
            lease,
            sequence,
            Some(owner),
            Command::Pause(PauseReason::Manual),
        )
        .await
    }

    async fn submit(
        &self,
        client: Uuid,
        lease: Uuid,
        sequence: u32,
        owner: Option<&str>,
        command: Command,
    ) -> ResultUpdate {
        if sequence == 0 {
            return Err("Invalid live capture sequence.".into());
        }
        let (reply, result) = oneshot::channel();
        {
            let slot = self.0.lock();
            let recording = slot
                .as_ref()
                .filter(|r| {
                    r.identity.client == client
                        && r.identity.lease == lease
                        && owner.is_none_or(|owner| owner == r.identity.tab_id)
                })
                .ok_or("The live capture lease has ended.")?;
            if *recording.cancel.borrow() {
                return Err("The live capture lease was cancelled.".into());
            }
            recording
                .commands
                .try_send(Request {
                    sequence,
                    command,
                    reply,
                })
                .map_err(|_| "Live capture could not accept another frame.")?;
        }
        result
            .await
            .map_err(|_| "The live capture lease has ended.")?
    }
}

impl ServiceContext {
    pub async fn live_capture_readiness(&self) -> codeterm_plugin::transcribe::TranscriberStatus {
        use codeterm_plugin::transcribe::TranscriberStatus;
        let registry = self.ext::<Arc<TranscribeRegistry>>();
        let preferred = self.configuration().integrations.voice_transcribe.active;
        let Some((backend, _)) = select_backend(registry, &preferred) else {
            return TranscriberStatus::Unavailable {
                reason: "Install a live dictation plugin.".into(),
            };
        };
        timeout(Duration::from_secs(2), backend.status())
            .await
            .unwrap_or_else(|_| TranscriberStatus::Unavailable {
                reason: "Transcriber readiness timed out.".into(),
            })
    }

    pub fn live_capture_sessions(&self) -> Arc<LiveCaptureSessions> {
        if let Some(sessions) = self.host_extension::<LiveCaptureSessions>() {
            return sessions;
        }
        let sessions = Arc::new(LiveCaptureSessions::default());
        if self.register_host_extension(sessions.clone()).is_ok() {
            return sessions;
        }
        self.host_extension::<LiveCaptureSessions>()
            .unwrap_or(sessions)
    }

    pub async fn start_live_capture(
        self: &Arc<Self>,
        client: Uuid,
        request: LiveCaptureStart,
    ) -> LiveCaptureAdmission {
        let unavailable = |reason: String| LiveCaptureAdmission::Unavailable { reason };
        let Some(tab) = TabIdentity::read(self, &request.tab_id) else {
            return unavailable("The live capture tab is no longer open.".into());
        };
        let Some(output) = self.host_extension::<LiveCaptureOutput>() else {
            return unavailable("Live capture storage is unavailable.".into());
        };
        let microphone = match self.ext::<DictationSessions>().claim_microphone() {
            Ok(lease) => lease,
            Err(error) => return unavailable(error),
        };
        let registry = self.ext::<Arc<TranscribeRegistry>>().clone();
        let preferred = self.configuration().integrations.voice_transcribe.active;
        let Some((backend, registration)) = select_backend(&registry, &preferred) else {
            return LiveCaptureAdmission::Setup {
                reason: "A live dictation plugin is needed on this device.".into(),
            };
        };
        if let Err(reason) = output.0.prepare(&request.tab_id).await {
            return unavailable(reason);
        }
        let identity = LiveCaptureIdentity {
            client,
            lease: Uuid::new_v4(),
            tab_id: request.tab_id.clone(),
        };
        let authority = Authority {
            svc: Arc::downgrade(self),
            registry,
            tab,
            tab_id: request.tab_id,
            plugin: backend.id().to_owned(),
            registration,
        };
        let mut claim = self.live_capture_sessions().claim(identity, microphone);
        let (reply, started) = oneshot::channel();
        tokio::spawn(async move {
            let engine = match open_engine(&backend).await {
                DictationAdmission::Ready(engine) => engine,
                DictationAdmission::Setup(reason) => {
                    drop(claim);
                    let _ = reply.send(LiveCaptureAdmission::Setup { reason });
                    return;
                }
                DictationAdmission::Unavailable(reason) => {
                    drop(claim);
                    let _ = reply.send(unavailable(reason));
                    return;
                }
            };
            let mut context = CaptureContext {
                identity: claim.identity.clone(),
                authority,
                preferred,
                cancel: claim.cancel.clone(),
                output,
            };
            if context.check().is_err()
                || reply
                    .send(LiveCaptureAdmission::Ready {
                        lease: claim.identity.lease,
                    })
                    .is_err()
            {
                engine.close().await;
                return;
            }
            let mut stream = CaptureStream::new(backend, engine, request.resume);
            let reply = stream.run(&mut claim, &mut context).await;
            stream.close().await;
            let current = context.check();
            drop(claim);
            if let Some((sender, result)) = reply {
                let _ = sender.send(current.and(result));
            }
        });
        started
            .await
            .unwrap_or_else(|_| unavailable("The live capture worker did not start.".into()))
    }
}

struct CaptureContext {
    identity: LiveCaptureIdentity,
    authority: Authority,
    preferred: String,
    cancel: watch::Receiver<bool>,
    output: Arc<LiveCaptureOutput>,
}

impl CaptureContext {
    fn check(&self) -> Result<(), String> {
        if *self.cancel.borrow() {
            return Err("The live capture lease was cancelled.".into());
        }
        if !self.authority.current()
            || !self.authority.svc.upgrade().is_some_and(|svc| {
                svc.ext::<crate::config::ConfigState>()
                    .lock()
                    .integrations
                    .voice_transcribe
                    .active
                    == self.preferred
            })
        {
            return Err("The live capture tab or transcription plugin changed.".into());
        }
        Ok(())
    }

    async fn append(&self, event: LiveCaptureEvent) -> Result<(), String> {
        self.check()?;
        self.output.0.append(&self.identity, event).await?;
        self.check()
    }
}

#[async_trait]
impl RotationOutput for CaptureContext {
    fn check(&self) -> Result<(), String> {
        CaptureContext::check(self)
    }

    fn cancellation(&self) -> &watch::Receiver<bool> {
        &self.cancel
    }

    async fn accept(&mut self, segment: DictationSegment, _: bool) -> Result<String, String> {
        if !segment.committed.is_empty() {
            self.append(LiveCaptureEvent::Speech {
                text: segment.committed,
            })
            .await?;
        }
        Ok(segment.provisional)
    }
}

struct CaptureStream {
    engine: RotatingEngine,
    resume: bool,
}

type Terminal = (oneshot::Sender<ResultUpdate>, ResultUpdate);

impl CaptureStream {
    fn new(backend: Arc<dyn Transcriber>, engine: Engine, resume: bool) -> Self {
        Self {
            engine: RotatingEngine::new(backend, engine),
            resume,
        }
    }

    async fn run(&mut self, claim: &mut Claim, context: &mut CaptureContext) -> Option<Terminal> {
        let mut next = 1u32;
        loop {
            let request = tokio::select! {
                biased;
                _ = cancelled(&mut claim.cancel) => return None,
                received = timeout(IDLE, claim.commands.recv()) => match received {
                    Ok(request) => request?,
                    Err(_) => {
                        if self.pause(context, PauseReason::Inactive).await.is_err() {
                            let _ = context.append(LiveCaptureEvent::Paused { reason: PauseReason::Failed }).await;
                        }
                        return None;
                    }
                },
            };
            if request.sequence != next || next == u32::MAX {
                let _ = request
                    .reply
                    .send(Err("The live capture frame is out of order.".into()));
                continue;
            }
            let paused = matches!(&request.command, Command::Pause(_));
            let result = match request.command {
                Command::Audio(pcm) => self.audio(context, pcm).await,
                Command::Pause(reason) => self.pause(context, reason).await.map(|()| String::new()),
            }
            .map(|provisional| LiveCaptureUpdate {
                lease: claim.identity.lease,
                sequence: next,
                provisional,
                paused,
            });
            if result.is_err() {
                self.close().await;
                let _ = context
                    .append(LiveCaptureEvent::Paused {
                        reason: PauseReason::Failed,
                    })
                    .await;
            }
            if paused || result.is_err() {
                return Some((request.reply, result));
            }
            let _ = request.reply.send(result);
            next += 1;
        }
    }

    async fn audio(
        &mut self,
        context: &mut CaptureContext,
        pcm: Vec<u8>,
    ) -> Result<String, String> {
        if self.resume {
            context.append(LiveCaptureEvent::Resumed).await?;
            self.resume = false;
        }
        self.engine.audio(context, pcm).await
    }

    async fn pause(
        &mut self,
        context: &mut CaptureContext,
        reason: PauseReason,
    ) -> Result<(), String> {
        self.engine.close_standby().await;
        self.engine.finish_engine(context).await?;
        context.append(LiveCaptureEvent::Paused { reason }).await
    }

    async fn close(&mut self) {
        self.engine.close().await;
    }
}

#[cfg(test)]
mod tests;
