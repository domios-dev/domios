use super::*;
use crate::agent::chat_types::{MessageBlock, RichMessage, SessionDelta};
use crate::agent::provider::{AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery};
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[test]
fn selects_exact_source_only_until_own_record_exists() {
    for (missing, source, expected) in [
        (true, Some("source"), Some("source")),
        (false, Some("source"), None),
        (true, None, None),
        (true, Some(""), None),
    ] {
        assert_eq!(inherited_source(missing, source), expected);
    }
}

struct ForkProvider;

impl AgentProvider for ForkProvider {
    fn id(&self) -> &str {
        "fork-fixture"
    }
    fn display_name(&self) -> &str {
        "Fork fixture"
    }
    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay {
            duration: Duration::ZERO,
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
    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        panic!("history must never guess a session")
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
        let data = std::fs::read(path).ok()?;
        let mut offset = from as usize;
        let end = data.len().min(offset.saturating_add(budget as usize));
        let mut messages = Vec::new();
        for line in data
            .get(offset..end)?
            .split_inclusive(|byte| *byte == b'\n')
        {
            if !line.ends_with(b"\n") {
                break;
            }
            let record: serde_json::Value = serde_json::from_slice(line).ok()?;
            messages.push(RichMessage {
                role: "user".into(),
                uuid: record["uuid"].as_str().map(str::to_owned),
                blocks: vec![MessageBlock {
                    kind: "text".into(),
                    data: serde_json::json!({"text": record["message"]["content"]}),
                }],
                record_start: Some(offset as u64),
                ..Default::default()
            });
            offset += line.len();
        }
        Some(SessionDelta {
            new_offset: offset as u64,
            messages,
        })
    }
}

fn write_rows(path: &Path, rows: &[(&str, &str)]) {
    let data: String = rows
        .iter()
        .map(|(id, content)| {
            format!(
                "{}\n",
                serde_json::json!({
                    "type": "user", "uuid": id, "message": {"role": "user", "content": content}
                })
            )
        })
        .collect();
    std::fs::write(path, data).unwrap();
}

#[test]
fn tab_snapshot_reads_inherited_then_only_fork_without_duplicates_or_evidence() {
    let root = tempfile::tempdir().unwrap();
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("fork-fixture".into());
    tab.agent_session_id = Some("fork".into());
    tab.fork_source_session_id = Some("source".into());
    tab.cwd = Some(root.path().to_string_lossy().into_owned());
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(ForkProvider));
    write_rows(&root.path().join("source.jsonl"), &[("inherited", "same")]);
    write_rows(&root.path().join("latest.jsonl"), &[("foreign", "wrong")]);
    let evidence_source = fixture.service.chat_registry.jsonl_source(&tab_id);
    let own_path = root.path().join("fork.jsonl");
    let super::super::registry::SourceCapture::Absent { witness: before } =
        evidence_source.capture("fork", &own_path)
    else {
        panic!("the fork's own transcript must be absent");
    };
    let (rows, count) = super::super::tab_chat_snapshot(&fixture.service, &tab_id, 0).unwrap();
    assert_eq!(count, 1);
    assert_eq!(rows[0].content, "same");
    assert_eq!(rows[0].inherited_from_session_id.as_deref(), Some("source"));
    let inherited = snapshot(
        &fixture.service,
        &tab_id,
        "fork-fixture",
        root.path().to_str().unwrap(),
        "fork",
        &root.path().join("fork.jsonl"),
    );
    assert_eq!(
        (
            inherited.total,
            inherited.start_offset,
            inherited.end_offset
        ),
        (0, 0, 0)
    );
    assert!(inherited.runtime_observations.is_empty());
    assert!(inherited.context_usage.is_none());
    let super::super::registry::SourceCapture::Absent { witness: after } =
        evidence_source.capture("fork", &own_path)
    else {
        panic!("inherited history must not materialize fork evidence");
    };
    assert_eq!(before, after);
    write_rows(
        &root.path().join("fork.jsonl"),
        &[("inherited", "same"), ("new", "next")],
    );
    let (rows, count) = super::super::tab_chat_snapshot(&fixture.service, &tab_id, 0).unwrap();
    assert_eq!(count, 2);
    assert_eq!(
        rows.iter()
            .map(|row| row.content.as_str())
            .collect::<Vec<_>>(),
        ["same", "next"]
    );
    assert!(rows
        .iter()
        .all(|row| row.inherited_from_session_id.is_none()));
    std::fs::write(root.path().join("fork.jsonl"), "").unwrap();
    let (rows, count) = super::super::tab_chat_snapshot(&fixture.service, &tab_id, 0).unwrap();
    assert_eq!((rows.len(), count), (0, 0));
}
