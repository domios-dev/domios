use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;
use async_trait::async_trait;
use codeterm_plugin::transcribe::{DictationSession, TranscribeError, Transcriber, TranscriberStatus, Transcript};
use codeterm_types::dictation::DictationSegment;

struct NativeFixture;

#[async_trait]
impl Transcriber for NativeFixture {
    fn id(&self) -> &str { "native" }
    fn supports_dictation(&self) -> bool { true }
    async fn status(&self) -> TranscriberStatus { TranscriberStatus::Ready }
    async fn start_dictation(&self) -> DictationAdmission {
        DictationAdmission::Ready(Box::new(Self))
    }
    async fn transcribe(&self, _path: &std::path::Path, _language: &str) -> Result<Transcript, TranscribeError> {
        Err(TranscribeError::Empty)
    }
}

#[async_trait]
impl DictationSession for NativeFixture {
    async fn update(&mut self, pcm: Vec<u8>, finish: bool) -> Result<DictationSegment, String> {
        Ok(if finish {
            DictationSegment { committed: "final speech".into(), provisional: String::new() }
        } else {
            assert_eq!(pcm, [7, 8]);
            DictationSegment { committed: String::new(), provisional: "interim speech".into() }
        })
    }

    async fn close(self: Box<Self>) {}
}

#[tokio::test]
async fn native_session_returns_pre_stop_text_and_finishes_once() {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.create_workspace("Fixture".into(), None);
    let tab = mux.create_tab(&workspace.id).unwrap();
    let fixture = ServiceFixture::new(mux);
    let svc = &fixture.service;
    svc.ext::<Arc<TranscribeRegistry>>().register("native", Arc::new(NativeFixture));
    assert!(matches!(svc.start_dictation("client", DictationStart { session: "session".into(), tab_id: tab.clone() }).await,
        DictationStartResult::Ready));
    let sessions = svc.ext::<DictationSessions>();
    assert!(sessions.frame("other", "session", 1, vec![7, 8], false).await.is_err());
    assert!(sessions.frame("client", "session", 2, vec![7, 8], false).await.is_err());
    assert!(sessions.frame("client", "session", 1, vec![0; MAX_PCM_BYTES + 2], false).await.is_err());
    let partial = sessions.frame("client", "session", 1, vec![7, 8], false).await.unwrap();
    assert_eq!((partial.session.as_str(), partial.sequence, partial.provisional.as_str(), partial.is_final),
        ("session", 1, "interim speech", false));
    assert!(sessions.frame("client", "session", 1, vec![7, 8], false).await.is_err());
    let final_update = sessions.frame("client", "session", 2, Vec::new(), true).await.unwrap();
    assert!(final_update.is_final);
    assert_eq!(final_update.committed, "final speech");
    assert!(sessions.frame("client", "session", 2, Vec::new(), true).await.is_err());
}

#[tokio::test]
async fn exact_cancellation_keeps_the_slot_until_its_owner_releases_it() {
    let sessions = DictationSessions::default();
    let mut claim = sessions.claim("client", "old").unwrap();
    sessions.cancel("other", "old");
    sessions.cancel("client", "new");
    assert!(!*claim.cancellation.borrow());
    sessions.cancel("client", "old");
    claim.cancellation.changed().await.unwrap();
    assert!(sessions.claim("client", "new").is_err());
    assert!(sessions.frame("client", "old", 1, vec![0, 0], false).await.is_err());
    drop(claim);
    let next = sessions.claim("client", "new").unwrap();
    sessions.cancel("client", "old");
    assert!(!*next.cancellation.borrow());
}
