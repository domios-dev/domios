use std::path::Path;
use std::sync::Arc;

use super::*;
use crate::chatpreprocessor::{ChatPreprocessorProvider, PreprocessCtx};

/// A fake text stage that appends a fixed tag — order is observable in the output.
struct TagStage {
    id: String,
    tag: String,
}

impl ChatPreprocessorProvider for TagStage {
    fn id(&self) -> &str {
        &self.id
    }
    fn preprocess(&self, _ctx: &PreprocessCtx, text: &str) -> Option<String> {
        Some(format!("{text}{}", self.tag))
    }
}

/// A fake stage that always passes through.
struct PassthroughStage;
impl ChatPreprocessorProvider for PassthroughStage {
    fn id(&self) -> &str {
        "passthrough"
    }
    fn preprocess(&self, _ctx: &PreprocessCtx, _text: &str) -> Option<String> {
        None
    }
}

#[test]
fn stages_run_in_order() {
    let stages: Vec<Arc<dyn ChatPreprocessorProvider>> = vec![
        Arc::new(TagStage {
            id: "a".into(),
            tag: "[a]".into(),
        }),
        Arc::new(TagStage {
            id: "b".into(),
            tag: "[b]".into(),
        }),
    ];
    let ctx = PreprocessCtx {
        pane_id: "p1".into(),
        ..Default::default()
    };
    // Slice order is the run order: a before b.
    assert_eq!(run_text_stages(&stages, &ctx, "x".into()), "x[a][b]");
}

#[test]
fn passthrough_stage_leaves_text_unchanged() {
    let stages: Vec<Arc<dyn ChatPreprocessorProvider>> = vec![Arc::new(PassthroughStage)];
    let ctx = PreprocessCtx::default();
    assert_eq!(run_text_stages(&stages, &ctx, "hello".into()), "hello");
}

#[test]
fn empty_pipeline_is_identity() {
    let stages: Vec<Arc<dyn ChatPreprocessorProvider>> = vec![];
    let ctx = PreprocessCtx::default();
    assert_eq!(run_text_stages(&stages, &ctx, "hello".into()), "hello");
}

#[test]
fn overlapping_filter_flagged() {
    // Two text stages claim the same slot → one conflict on Text.
    let conflicts = detect_conflicts([("pre1", StageMatch::Text), ("pre2", StageMatch::Text)]);
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].slot, StageMatch::Text);
    assert_eq!(
        conflicts[0].ids,
        vec!["pre1".to_string(), "pre2".to_string()]
    );
}

#[test]
fn voice_and_text_do_not_collide() {
    // The transcriber (voice) and a text preprocessor never collide — they chain.
    let conflicts = detect_conflicts([
        ("transcriber", StageMatch::Voice),
        ("pre", StageMatch::Text),
    ]);
    assert!(conflicts.is_empty());
}

#[test]
fn two_voice_stages_flagged() {
    let conflicts = detect_conflicts([("t1", StageMatch::Voice), ("t2", StageMatch::Voice)]);
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].slot, StageMatch::Voice);
}

// --- Transcriber-as-voice-stage: prove the recast preserves transcription ---

struct FakeTranscriber;

#[async_trait::async_trait]
impl crate::transcribe::Transcriber for FakeTranscriber {
    fn id(&self) -> &str {
        "fake"
    }
    async fn status(&self) -> crate::transcribe::TranscriberStatus {
        crate::transcribe::TranscriberStatus::Ready
    }
    async fn transcribe(
        &self,
        _audio_path: &Path,
        language: &str,
    ) -> Result<crate::transcribe::Transcript, crate::transcribe::TranscribeError> {
        Ok(crate::transcribe::Transcript {
            text: format!("hello {language}"),
        })
    }
}

#[tokio::test]
async fn voice_stage_transcribes_via_active_engine() {
    let reg = crate::transcribe::TranscribeRegistry::new();
    reg.register("fake", Arc::new(FakeTranscriber));
    let out = run_voice_stage(&reg, "fake", Path::new("/a.oga"), "ru")
        .await
        .unwrap();
    assert_eq!(out.text, "hello ru");
}

#[tokio::test]
async fn voice_stage_auto_selects_single_registered_engine_when_active_empty() {
    let reg = crate::transcribe::TranscribeRegistry::new();
    reg.register("fake", Arc::new(FakeTranscriber));
    let out = run_voice_stage(&reg, "", Path::new("/a.oga"), "ru")
        .await
        .unwrap();
    assert_eq!(out.text, "hello ru");
}

#[tokio::test]
async fn voice_stage_no_backend_errors() {
    let reg = crate::transcribe::TranscribeRegistry::new();
    assert!(matches!(
        run_voice_stage(&reg, "missing", Path::new("/a.oga"), "").await,
        Err(crate::transcribe::TranscribeError::NoBackend)
    ));
}

#[test]
fn user_turn_send_runs_stages_only_for_user_agent_sends() {
    // args: (is_service_push, has_source_pane, is_chat_backed, is_agent_pane)
    // P3/F3: a plain user send to a PTY AGENT pane runs the stages.
    assert!(is_user_turn_send(false, false, false, true));
    // BUG A: a SHELL pane (no agent_type) must NEVER get a context block —
    // injecting one garbled `echo X` and broke 23 e2e tests.
    assert!(!is_user_turn_send(false, false, false, false));
    // Service push (svc_token) is system traffic → skip.
    assert!(!is_user_turn_send(true, false, false, true));
    // Carries a source_pane (inter-pane / mesh / report delivery) → skip.
    assert!(!is_user_turn_send(false, true, false, true));
    // chatBackend pane → its transport already staged; don't double-process.
    assert!(!is_user_turn_send(false, false, true, true));
}
