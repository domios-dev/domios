use std::{
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};

use codeterm_plugin::transcribe::{TranscribeError, TranscriberStatus, Transcript};
use codeterm_types::dictation::DictationStart;
use parking_lot::Mutex;
use tokio::sync::Notify;

use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};

use super::*;

#[derive(Default)]
struct Sink(Mutex<Vec<(LiveCaptureIdentity, LiveCaptureEvent)>>);

#[async_trait]
impl LiveCaptureSink for Sink {
    async fn append(
        &self,
        capture: &LiveCaptureIdentity,
        event: LiveCaptureEvent,
    ) -> Result<(), String> {
        self.0.lock().push((capture.clone(), event));
        Ok(())
    }
}

#[derive(Default)]
struct Probe {
    exclusive: bool,
    fail_finish: bool,
    active: AtomicUsize,
    peak: AtomicUsize,
    opened: AtomicUsize,
    standbys: AtomicUsize,
    standby_seen: Notify,
    endings: Mutex<Vec<(usize, usize)>>,
    finish_gate: Option<(Notify, Notify)>,
}

struct Provider(Arc<Probe>);

#[async_trait]
impl Transcriber for Provider {
    fn id(&self) -> &str {
        "capture-fixture"
    }

    fn supports_dictation(&self) -> bool {
        true
    }

    async fn status(&self) -> TranscriberStatus {
        TranscriberStatus::Ready
    }

    async fn start_dictation(&self) -> DictationAdmission {
        let active = self.0.active.load(Ordering::SeqCst);
        let standby = active != 0;
        let refused = standby && self.0.exclusive;
        let index = if refused {
            0
        } else {
            let active = self.0.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.0.peak.fetch_max(active, Ordering::SeqCst);
            self.0.opened.fetch_add(1, Ordering::SeqCst) + 1
        };
        DictationAdmission::Ready(Box::new(FakeEngine {
            probe: self.0.clone(),
            index,
            standby,
            samples: 0,
        }))
    }

    async fn transcribe(&self, _: &Path, _: &str) -> Result<Transcript, TranscribeError> {
        Err(TranscribeError::Empty)
    }
}

struct FakeEngine {
    probe: Arc<Probe>,
    index: usize,
    standby: bool,
    samples: usize,
}

impl Drop for FakeEngine {
    fn drop(&mut self) {
        if self.index != 0 {
            self.probe.active.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

#[async_trait]
impl DictationSession for FakeEngine {
    async fn update(&mut self, pcm: Vec<u8>, finish: bool) -> Result<DictationSegment, String> {
        if pcm == [0; VAD_WINDOW * 2] {
            if self.standby {
                self.probe.standbys.fetch_add(1, Ordering::SeqCst);
                self.probe.standby_seen.notify_one();
            }
            return if self.index == 0 {
                Err("Inference admission is exclusive.".into())
            } else {
                Ok(DictationSegment::default())
            };
        }
        if finish {
            if self.probe.fail_finish {
                return Err("The engine failed during its final update.".into());
            }
            if let Some((entered, release)) = &self.probe.finish_gate {
                entered.notify_one();
                release.notified().await;
            }
            self.probe.endings.lock().push((self.index, self.samples));
            return Ok(DictationSegment {
                committed: format!("tail {}", self.index),
                provisional: String::new(),
            });
        }
        if self.index == 0 {
            return Err("The engine was not admitted.".into());
        }
        self.samples += pcm.len() / 2;
        Ok(DictationSegment {
            committed: if self.probe.fail_finish {
                "durable prefix".into()
            } else {
                String::new()
            },
            provisional: "interim".into(),
        })
    }

    async fn close(self: Box<Self>) {}
}

fn fixture(probe: Arc<Probe>) -> (ServiceFixture, Arc<ServiceContext>, String, Arc<Sink>) {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.create_workspace("Fixture".into(), None);
    let tab = mux.create_tab(&workspace.id).unwrap();
    let fixture = ServiceFixture::new(mux);
    let svc = Arc::new(fixture.service.clone());
    svc.ext::<Arc<TranscribeRegistry>>()
        .register("capture-fixture", Arc::new(Provider(probe)));
    let sink = Arc::new(Sink::default());
    assert!(svc
        .register_host_extension(Arc::new(LiveCaptureOutput(sink.clone())))
        .is_ok());
    (fixture, svc, tab, sink)
}

async fn start(svc: &Arc<ServiceContext>, owner: Uuid, tab: &str, resume: bool) -> Uuid {
    match svc
        .start_live_capture(
            owner,
            LiveCaptureStart {
                tab_id: tab.into(),
                resume,
            },
        )
        .await
    {
        LiveCaptureAdmission::Ready { lease } => lease,
        state => panic!("expected live capture admission, got {state:?}"),
    }
}

fn spoken(samples: usize) -> Vec<u8> {
    2000i16.to_le_bytes().repeat(samples)
}

#[tokio::test]
async fn live_capture_uses_ready_standby_and_exact_silence_then_preserves_tail_order() {
    let probe = Arc::new(Probe::default());
    let (_fixture, svc, tab, sink) = fixture(probe.clone());
    let owner = Uuid::new_v4();
    let lease = start(&svc, owner, &tab, false).await;
    let sessions = svc.live_capture_sessions();
    let mut sequence = 0;
    let speech = spoken(MAX_PCM_BYTES / 2);
    for _ in 0..(PAUSE_FROM * 2 / MAX_PCM_BYTES) {
        sequence += 1;
        sessions
            .frame(owner, lease, sequence, speech.clone())
            .await
            .unwrap();
    }
    timeout(Duration::from_secs(2), probe.standby_seen.notified())
        .await
        .unwrap();
    sequence += 1;
    sessions
        .frame(owner, lease, sequence, vec![0; MAX_PCM_BYTES])
        .await
        .unwrap();
    assert!(
        probe.endings.lock().is_empty(),
        "500 ms does not meet a 600 ms boundary"
    );
    sequence += 1;
    let mut boundary_frame = vec![0; SILENCE * 2 - MAX_PCM_BYTES];
    boundary_frame.extend(spoken(VAD_WINDOW));
    sessions
        .frame(owner, lease, sequence, boundary_frame)
        .await
        .unwrap();
    assert_eq!(*probe.endings.lock(), [(1, PAUSE_FROM + SILENCE)]);
    sequence += 1;
    sessions
        .frame(owner, lease, sequence, speech)
        .await
        .unwrap();
    let update = sessions
        .pause(owner, lease, sequence + 1, PauseReason::FocusLost)
        .await
        .unwrap();
    assert!(update.paused && update.provisional.is_empty());
    assert_eq!(
        *probe.endings.lock(),
        [
            (1, PAUSE_FROM + SILENCE),
            (2, MAX_PCM_BYTES / 2 + VAD_WINDOW)
        ]
    );
    assert_eq!(probe.peak.load(Ordering::SeqCst), 2);
    assert_eq!(probe.active.load(Ordering::SeqCst), 0);
    let events = sink.0.lock();
    assert!(events
        .iter()
        .all(|(id, _)| id.lease == lease && id.client == owner && id.tab_id == tab));
    assert_eq!(
        events.iter().map(|(_, event)| event).collect::<Vec<_>>(),
        [
            &LiveCaptureEvent::Speech {
                text: "tail 1".into()
            },
            &LiveCaptureEvent::Speech {
                text: "tail 2".into()
            },
            &LiveCaptureEvent::Paused {
                reason: PauseReason::FocusLost
            },
        ]
    );
}

#[tokio::test]
async fn live_capture_exclusive_handoff_preserves_all_samples_across_two_hard_rotations() {
    let probe = Arc::new(Probe {
        exclusive: true,
        ..Default::default()
    });
    let (_fixture, svc, tab, sink) = fixture(probe.clone());
    let owner = Uuid::new_v4();
    let lease = start(&svc, owner, &tab, true).await;
    let sessions = svc.live_capture_sessions();
    let speech = spoken(MAX_PCM_BYTES / 2);
    sessions.frame(owner, lease, 1, spoken(97)).await.unwrap();
    let whole_frames = HARD_BOUNDARY * 4 / MAX_PCM_BYTES;
    for index in 0..whole_frames {
        let sequence = u32::try_from(index).unwrap() + 2;
        sessions
            .frame(owner, lease, sequence, speech.clone())
            .await
            .unwrap();
    }
    sessions
        .pause(
            owner,
            lease,
            u32::try_from(whole_frames).unwrap() + 2,
            PauseReason::Manual,
        )
        .await
        .unwrap();
    assert_eq!(probe.standbys.load(Ordering::SeqCst), 1);
    assert_eq!(probe.peak.load(Ordering::SeqCst), 1);
    assert_eq!(
        *probe.endings.lock(),
        [(1, HARD_BOUNDARY), (2, HARD_BOUNDARY), (3, 97)]
    );
    let events = sink.0.lock();
    assert_eq!(
        events.iter().map(|(_, event)| event).collect::<Vec<_>>(),
        [
            &LiveCaptureEvent::Resumed,
            &LiveCaptureEvent::Speech {
                text: "tail 1".into()
            },
            &LiveCaptureEvent::Speech {
                text: "tail 2".into()
            },
            &LiveCaptureEvent::Speech {
                text: "tail 3".into()
            },
            &LiveCaptureEvent::Paused {
                reason: PauseReason::Manual
            },
        ]
    );
}

#[tokio::test]
async fn live_capture_arbitrates_atomic_admission_and_fences_a_cancelled_finish() {
    let probe = Arc::new(Probe {
        finish_gate: Some((Notify::new(), Notify::new())),
        ..Default::default()
    });
    let (_fixture, svc, tab, sink) = fixture(probe.clone());
    let owner = Uuid::new_v4();
    let other = Uuid::new_v4();
    let left = svc.start_live_capture(
        owner,
        LiveCaptureStart {
            tab_id: tab.clone(),
            resume: false,
        },
    );
    let right = svc.start_live_capture(
        other,
        LiveCaptureStart {
            tab_id: tab.clone(),
            resume: false,
        },
    );
    let (left, right) = tokio::join!(left, right);
    let (owner, lease, foreign) = match (left, right) {
        (LiveCaptureAdmission::Ready { lease }, LiveCaptureAdmission::Unavailable { .. }) => {
            (owner, lease, other)
        }
        (LiveCaptureAdmission::Unavailable { .. }, LiveCaptureAdmission::Ready { lease }) => {
            (other, lease, owner)
        }
        pair => panic!("expected a single admission, got {pair:?}"),
    };
    let composer = svc.ext::<DictationSessions>();
    let request = || DictationStart {
        tab_id: tab.clone(),
        session: "composer".into(),
    };
    assert!(matches!(
        svc.start_dictation("composer", request()).await,
        super::super::DictationStartResult::Unavailable { .. }
    ));
    let sessions = svc.live_capture_sessions();
    assert!(sessions.frame(foreign, lease, 1, spoken(32)).await.is_err());
    assert!(sessions
        .frame(owner, lease, 1, vec![0; MAX_PCM_BYTES + 2])
        .await
        .is_err());
    assert!(sessions.frame(owner, lease, 2, spoken(32)).await.is_err());
    assert!(sessions.frame(owner, lease, 1, spoken(32)).await.is_ok());
    let pausing = {
        let sessions = sessions.clone();
        tokio::spawn(async move { sessions.pause(owner, lease, 2, PauseReason::Hidden).await })
    };
    let (entered, _) = probe.finish_gate.as_ref().unwrap();
    timeout(Duration::from_secs(2), entered.notified())
        .await
        .unwrap();
    sessions.cancel(foreign, lease);
    assert!(composer.claim("composer", "composer").is_err());
    sessions.cancel(owner, lease);
    assert!(pausing.await.unwrap().is_err());
    assert!(
        sink.0.lock().is_empty(),
        "a cancelled final callback cannot publish a tail or marker"
    );
    let composer_claim = composer.claim("composer", "composer").unwrap();
    assert!(matches!(
        svc.start_live_capture(
            owner,
            LiveCaptureStart {
                tab_id: tab.clone(),
                resume: false
            }
        )
        .await,
        LiveCaptureAdmission::Unavailable { .. }
    ));
    drop(composer_claim);
    let next = start(&svc, owner, &tab, true).await;
    assert_ne!(lease, next);
    sessions.cancel(owner, lease);
    assert!(sessions.frame(owner, next, 1, spoken(32)).await.is_ok());
    sessions.cancel(owner, next);
}

#[tokio::test]
async fn live_capture_plugin_replacement_fences_the_final_callback() {
    let probe = Arc::new(Probe {
        finish_gate: Some((Notify::new(), Notify::new())),
        ..Default::default()
    });
    let (_fixture, svc, tab, sink) = fixture(probe.clone());
    let owner = Uuid::new_v4();
    let lease = start(&svc, owner, &tab, false).await;
    let sessions = svc.live_capture_sessions();
    let pausing =
        tokio::spawn(async move { sessions.pause(owner, lease, 1, PauseReason::Hidden).await });
    let (entered, release) = probe.finish_gate.as_ref().unwrap();
    timeout(Duration::from_secs(2), entered.notified())
        .await
        .unwrap();
    svc.ext::<Arc<TranscribeRegistry>>().register(
        "capture-fixture",
        Arc::new(Provider(Arc::new(Probe::default()))),
    );
    release.notify_one();
    assert!(pausing.await.unwrap().is_err());
    assert!(sink.0.lock().is_empty());
}

#[tokio::test]
async fn live_capture_reports_a_failed_flush_after_the_committed_prefix() {
    let (_fixture, svc, tab, sink) = fixture(Arc::new(Probe {
        fail_finish: true,
        ..Default::default()
    }));
    let owner = Uuid::new_v4();
    let lease = start(&svc, owner, &tab, false).await;
    let sessions = svc.live_capture_sessions();
    let update = sessions.frame(owner, lease, 1, spoken(1024)).await.unwrap();
    assert_eq!(update.provisional, "interim");
    let error = sessions
        .pause(owner, lease, 2, PauseReason::Hidden)
        .await
        .unwrap_err();
    assert_eq!(error, "The engine failed during its final update.");
    assert!(sessions
        .pause(owner, lease, 2, PauseReason::Hidden)
        .await
        .is_err());
    let events = sink.0.lock();
    assert_eq!(
        events.iter().map(|(_, event)| event).collect::<Vec<_>>(),
        [
            &LiveCaptureEvent::Speech {
                text: "durable prefix".into()
            },
            &LiveCaptureEvent::Paused {
                reason: PauseReason::Failed
            },
        ]
    );
}
