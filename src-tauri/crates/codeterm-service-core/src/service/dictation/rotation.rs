use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use codeterm_types::dictation::{DictationSegment, SAMPLE_RATE};
use tokio::{sync::watch, task::JoinHandle, time::timeout};

use super::{DictationAdmission, DictationSession, Transcriber, EXCHANGE, MAX_UPDATE_BYTES};

const RATE: usize = SAMPLE_RATE as usize;
pub(super) const WARM_AT: usize = RATE * 150;
pub(super) const PAUSE_FROM: usize = RATE * 180;
const PAUSE_TARGET_END: usize = RATE * 240;
pub(super) const HARD_BOUNDARY: usize = RATE * 270;
pub(super) const SILENCE: usize = RATE * 6 / 10;
pub(super) const VAD_WINDOW: usize = RATE / 100;
const SPEECH_RMS: i64 = 180;
const ADMIT: Duration = Duration::from_secs(18);

pub(super) type Engine = Box<dyn DictationSession>;

#[async_trait]
pub(super) trait RotationOutput: Send {
    fn check(&self) -> Result<(), String>;
    fn cancellation(&self) -> &watch::Receiver<bool>;
    async fn accept(&mut self, segment: DictationSegment, finished: bool)
        -> Result<String, String>;
}

async fn exchange(
    output: &mut impl RotationOutput,
    engine: &mut Engine,
    pcm: Vec<u8>,
) -> Result<String, String> {
    output.check()?;
    let mut cancel = output.cancellation().clone();
    let finish = pcm.is_empty();
    let segment = tokio::select! {
        biased;
        _ = cancelled(&mut cancel) => return Err("The transcription session was cancelled.".into()),
        reply = timeout(EXCHANGE, engine.update(pcm, finish)) => {
            reply.map_err(|_| "The transcription worker did not answer in time.")??
        }
    };
    output.check()?;
    if segment.committed.len() + segment.provisional.len() > MAX_UPDATE_BYTES {
        return Err("The transcription worker exceeded its transcript limit.".into());
    }
    output.accept(segment, finish).await
}

pub(super) async fn cancelled(cancel: &mut watch::Receiver<bool>) {
    let _ = cancel.wait_for(|value| *value).await;
}

pub(super) async fn open_engine(backend: &Arc<dyn Transcriber>) -> DictationAdmission {
    let admission = timeout(ADMIT, backend.start_dictation())
        .await
        .unwrap_or_else(|_| {
            DictationAdmission::Unavailable(
                "The live transcription worker did not start in time.".into(),
            )
        });
    let DictationAdmission::Ready(mut engine) = admission else {
        return admission;
    };
    // Allocated process pipes alone do not prove inference admission.
    let result = timeout(EXCHANGE, engine.update(vec![0; VAD_WINDOW * 2], false)).await;
    let reason = match result {
        Ok(Ok(segment)) if segment.committed.is_empty() && segment.provisional.is_empty() => {
            return DictationAdmission::Ready(engine);
        }
        Ok(Ok(_)) => "The transcription worker returned speech before capture started.".into(),
        Ok(Err(reason)) => reason,
        Err(_) => "The transcription worker did not acknowledge admission.".into(),
    };
    engine.close().await;
    DictationAdmission::Unavailable(reason)
}

struct Standby(JoinHandle<Option<Engine>>);

impl Drop for Standby {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl Standby {
    fn start(backend: Arc<dyn Transcriber>) -> Self {
        Self(tokio::spawn(async move {
            match open_engine(&backend).await {
                DictationAdmission::Ready(engine) => Some(engine),
                _ => None,
            }
        }))
    }

    async fn take_ready(mut self) -> Option<Engine> {
        if !self.0.is_finished() {
            self.0.abort();
        }
        (&mut self.0).await.ok().flatten()
    }

    async fn close(self) {
        if let Some(engine) = self.take_ready().await {
            engine.close().await;
        }
    }
}

#[derive(Default)]
struct Boundary {
    samples: usize,
    silence: usize,
    window_len: usize,
    energy: i64,
}

impl Boundary {
    fn consume(&mut self, pcm: &[u8], pause_from: usize) -> (usize, bool) {
        let (pairs, _) = pcm.as_chunks::<2>();
        for (index, &pair) in pairs.iter().enumerate() {
            let sample = i64::from(i16::from_le_bytes(pair));
            self.energy += sample * sample;
            self.samples += 1;
            self.window_len += 1;
            if self.window_len == VAD_WINDOW {
                self.silence = if self.energy < SPEECH_RMS * SPEECH_RMS * VAD_WINDOW as i64 {
                    self.silence + VAD_WINDOW
                } else {
                    0
                };
                self.window_len = 0;
                self.energy = 0;
                if self.samples >= pause_from && self.silence >= SILENCE {
                    return ((index + 1) * 2, true);
                }
            }
            if self.samples == HARD_BOUNDARY {
                return ((index + 1) * 2, true);
            }
        }
        (pcm.len(), false)
    }
}

pub(super) struct RotatingEngine {
    backend: Arc<dyn Transcriber>,
    engine: Option<Engine>,
    warming: Option<Standby>,
    try_standby: bool,
    boundary: Boundary,
}

impl RotatingEngine {
    pub(super) fn new(backend: Arc<dyn Transcriber>, engine: Engine) -> Self {
        Self {
            backend,
            engine: Some(engine),
            warming: None,
            try_standby: true,
            boundary: Boundary::default(),
        }
    }

    pub(super) async fn audio(
        &mut self,
        output: &mut impl RotationOutput,
        mut pcm: Vec<u8>,
    ) -> Result<String, String> {
        output.check()?;
        let mut provisional;
        loop {
            if self.engine.is_none() {
                let mut cancel = output.cancellation().clone();
                self.engine = Some(tokio::select! {
                    biased;
                    _ = cancelled(&mut cancel) => return Err("The transcription session was cancelled.".into()),
                    admission = open_engine(&self.backend) => match admission {
                        DictationAdmission::Ready(engine) => engine,
                        DictationAdmission::Setup(reason) | DictationAdmission::Unavailable(reason) => return Err(reason),
                    },
                });
            }
            let warming = self
                .warming
                .as_ref()
                .is_some_and(|standby| !standby.0.is_finished());
            let pause_from = if warming {
                PAUSE_TARGET_END
            } else {
                PAUSE_FROM
            };
            let (prefix, rotate) = self.boundary.consume(&pcm, pause_from);
            let rest = if pcm.len() > prefix {
                pcm.split_off(prefix)
            } else {
                Vec::new()
            };
            if self.try_standby && self.warming.is_none() && self.boundary.samples >= WARM_AT {
                self.warming = Some(Standby::start(self.backend.clone()));
            }
            let engine = self
                .engine
                .as_mut()
                .ok_or("The transcription engine is missing.")?;
            provisional = exchange(output, engine, pcm).await?;
            if rotate {
                self.rotate(output).await?;
                provisional.clear();
            }
            if rest.is_empty() {
                return Ok(provisional);
            }
            pcm = rest;
        }
    }

    pub(super) async fn finish_engine(
        &mut self,
        output: &mut impl RotationOutput,
    ) -> Result<(), String> {
        let Some(mut engine) = self.engine.take() else {
            return output.check();
        };
        let result = exchange(output, &mut engine, Vec::new()).await;
        engine.close().await;
        result?;
        output.check()
    }

    async fn rotate(&mut self, output: &mut impl RotationOutput) -> Result<(), String> {
        let finished = self.finish_engine(output).await;
        if let Some(standby) = self.warming.take() {
            self.engine = standby.take_ready().await;
            self.try_standby = self.engine.is_some();
        }
        finished?;
        self.boundary = Boundary::default();
        Ok(())
    }

    pub(super) async fn close(&mut self) {
        self.close_standby().await;
        if let Some(engine) = self.engine.take() {
            engine.close().await;
        }
    }

    pub(super) async fn close_standby(&mut self) {
        if let Some(standby) = self.warming.take() {
            standby.close().await;
        }
    }
}
