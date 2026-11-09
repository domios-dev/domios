use super::*;
use crate::agent::{
    chat_types::{MessageBlock, RichMessage, SessionDelta},
    provider::{AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery},
};
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;
use std::io::{BufRead, Seek, SeekFrom, Write};
use std::path::Path;

struct Provider(Arc<Mutex<Vec<u64>>>);

impl AgentProvider for Provider {
    fn id(&self) -> &str {
        "preview-index-fixture"
    }
    fn display_name(&self) -> &str {
        "Preview index fixture"
    }
    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay {
            duration: std::time::Duration::ZERO,
        }
    }
    fn task_delivery(&self) -> TaskDelivery {
        TaskDelivery::PtyInput
    }
    fn detect_from_title(&self, _: &str) -> bool {
        false
    }
    fn detect_from_output(&self, _: &str) -> bool {
        false
    }
    fn session_file_path(&self, cwd: &str, session: &str) -> Option<PathBuf> {
        Some(Path::new(cwd).join(format!("{session}.jsonl")))
    }
    fn session_jsonl_path(&self, cwd: &str, session: &str) -> Option<PathBuf> {
        self.session_file_path(cwd, session)
    }
    fn detect_session_id(
        &self,
        _: &str,
        _: &[String],
        _: Option<std::time::Duration>,
    ) -> Option<String> {
        panic!("must use own session")
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
    fn parse_session_delta_bounded(
        &self,
        path: &Path,
        from: u64,
        budget: u64,
    ) -> Option<SessionDelta> {
        self.0.lock().push(from);
        assert!(budget <= CHUNK_BYTES);
        let mut file = std::fs::File::open(path).ok()?;
        file.seek(SeekFrom::Start(from)).ok()?;
        let mut reader = std::io::BufReader::new(file);
        let mut offset = from;
        let mut messages = Vec::new();
        while offset - from < budget {
            let mut line = String::new();
            if reader.read_line(&mut line).ok()? == 0 || !line.ends_with('\n') {
                break;
            }
            let content: String = serde_json::from_str(&line).ok()?;
            messages.push(RichMessage {
                role: "assistant".into(),
                record_start: Some(offset),
                blocks: vec![MessageBlock {
                    kind: "text".into(),
                    data: serde_json::json!({"text":content}),
                }],
                ..Default::default()
            });
            offset += line.len() as u64;
        }
        Some(SessionDelta {
            messages,
            new_offset: offset,
        })
    }
}

fn append(path: &Path, content: &str) {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    writeln!(file, "{}", serde_json::to_string(content).unwrap()).unwrap();
}

fn fixture() -> (ServiceFixture, String, Arc<Mutex<Vec<u64>>>) {
    let (mut mux, tab) = MultiplexerState::new();
    mux.get_tab_mut(&tab).unwrap().agent_type = Some("preview-index-fixture".into());
    mux.get_tab_mut(&tab).unwrap().agent_session_id = Some("own".into());
    mux.get_tab_mut(&tab).unwrap().session_binding_nonce = Some("nonce".into());
    let fixture = ServiceFixture::new(mux);
    fixture.service.mux.write().get_tab_mut(&tab).unwrap().cwd =
        Some(fixture.temp.path().to_string_lossy().into_owned());
    let reads = Arc::new(Mutex::new(Vec::new()));
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(Provider(reads.clone())));
    (fixture, tab, reads)
}

fn index(svc: &ServiceContext, tab: &str) -> Arc<Entry> {
    let entry = svc
        .chat_registry
        .preview_references
        .entry(tab, source(svc, tab).unwrap().unwrap())
        .unwrap();
    build(svc, tab, &entry).unwrap();
    entry.state.lock().complete = true;
    entry
}

#[test]
fn historical_reference_previews_beyond_tail_and_warm_reads_never_parse() {
    let (fixture, tab, reads) = fixture();
    let artifacts = tempfile::tempdir().unwrap();
    let root = codeterm_local_preview::html_preview::preview_test_root(artifacts.path());
    let historical = root.join("apple2.png");
    let unreferenced = root.join("og2.png");
    std::fs::write(&historical, b"image").unwrap();
    std::fs::write(&unreferenced, b"other").unwrap();
    let transcript = fixture.temp.path().join("own.jsonl");
    append(
        &transcript,
        &codeterm_local_preview::artifact_directive::serialize_artifact(
            "image",
            &[historical.to_string_lossy().into_owned()],
        )
        .unwrap(),
    );
    for _ in 0..200 {
        append(&transcript, &"opaque".repeat(2048));
    }
    let (tail, _) = crate::service::tab_chat::tab_chat_snapshot(&fixture.service, &tab, 0).unwrap();
    assert!(!references::referenced_paths(&tail, None).contains(&references::path_key(&historical)));
    reads.lock().clear();
    let entry = index(&fixture.service, &tab);
    assert!(reads.lock().len() > 1);
    let parsed = reads.lock().len();
    assert_eq!(
        super::super::read(
            &fixture.service,
            &tab,
            &[],
            historical.to_str().unwrap(),
            "image"
        )
        .unwrap()
        .data,
        "data:image/png;base64,aW1hZ2U="
    );
    assert_eq!(
        super::super::read(
            &fixture.service,
            &tab,
            &[],
            unreferenced.to_str().unwrap(),
            "image"
        )
        .unwrap_err(),
        "Preview path is not referenced"
    );
    assert_eq!(reads.lock().len(), parsed);
    let end = entry.state.lock().offset;
    append(&transcript, "no new references");
    index(&fixture.service, &tab);
    assert_eq!(*reads.lock().last().unwrap(), end);
    assert!(lookup(&fixture.service, &tab)
        .unwrap()
        .unwrap()
        .contains(&references::path_key(&historical)));
}

#[test]
fn replacement_binding_change_and_shutdown_revoke_indexed_references() {
    let (fixture, tab, _) = fixture();
    let transcript = fixture.temp.path().join("own.jsonl");
    let referenced = fixture.temp.path().join("referenced.png");
    append(
        &transcript,
        &codeterm_local_preview::artifact_directive::serialize_artifact(
            "image",
            &[referenced.to_string_lossy().into_owned()],
        )
        .unwrap(),
    );
    let original = index(&fixture.service, &tab);
    std::fs::remove_file(&transcript).unwrap();
    append(&transcript, "unreferenced replacement");
    assert!(lookup(&fixture.service, &tab).is_err());
    let replaced = index(&fixture.service, &tab);
    assert!(replaced.state.lock().references.is_empty());
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .session_binding_nonce = Some("new-nonce".into());
    assert!(lookup(&fixture.service, &tab).is_err());
    assert!(original.cancelled.load(Ordering::Acquire));
    fixture.service.chat_registry.shutdown_tab(&tab);
    assert!(fixture
        .service
        .chat_registry
        .preview_references
        .entries
        .lock()
        .is_empty());
}

#[test]
fn inventory_still_denies_referenced_private_and_secret_paths() {
    let (fixture, tab, _) = fixture();
    let root = codeterm_local_preview::html_preview::preview_test_root(fixture.temp.path());
    let secret = root.join("auth-token.png");
    let private = root.join("private.png");
    for path in [&secret, &private] {
        std::fs::write(path, b"private").unwrap();
    }
    append(
        &fixture.temp.path().join("own.jsonl"),
        &codeterm_local_preview::artifact_directive::serialize_artifact(
            "image",
            &[
                secret.to_string_lossy().into_owned(),
                private.to_string_lossy().into_owned(),
            ],
        )
        .unwrap(),
    );
    index(&fixture.service, &tab);
    for path in [&secret, &private] {
        assert_eq!(
            super::super::read(
                &fixture.service,
                &tab,
                &[private.to_string_lossy().into_owned()],
                path.to_str().unwrap(),
                "image"
            )
            .unwrap_err(),
            "Preview path is private"
        );
    }
}

#[test]
fn empty_check_never_reads_transcript_or_allocates_an_index() {
    let (fixture, tab, reads) = fixture();
    let result = super::super::check(&fixture.service, &tab, &[], Vec::new()).unwrap();
    assert!(result.paths.is_empty());
    assert!(reads.lock().is_empty());
    assert!(fixture
        .service
        .chat_registry
        .preview_references
        .entries
        .lock()
        .is_empty());
}
