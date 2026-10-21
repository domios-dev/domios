use std::path::{Path, PathBuf};
use std::time::Duration;

use super::*;
use crate::agent::chat_types::{MessageBlock, RichMessage, SessionDelta};
use crate::agent::provider::{AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery};
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{ContextEpochArchive, ContextEpochBoundary};
use crate::service::tab_chat::chatbackend_history::{self, ChatBackendHistory};
use crate::service::test_fixture::ServiceFixture;

#[test]
fn only_a_truncated_window_gets_a_gap_naming_its_own_source_and_offset() {
    let rows = vec![ChatMsg {
        id: "row".into(),
        ..Default::default()
    }];
    assert_eq!(
        bounded(ChatHistorySource::ChatBackend, "s", 0, rows.clone()),
        rows
    );
    let gapped = bounded(ChatHistorySource::ProviderSession, "s", 42, rows);
    assert_eq!(gapped.len(), 2);
    assert_eq!(gapped[0].system_subtype.as_deref(), Some(SUBTYPE));
    assert_eq!(
        gapped[0].timeline_marker,
        Some(ChatTimelineMarker::HistoryGap {
            source: ChatHistorySource::ProviderSession,
            session_id: "s".into(),
            before_offset: 42,
        })
    );
    assert_eq!(gapped[1].id, "row");
}

struct PagedProvider;

impl AgentProvider for PagedProvider {
    fn id(&self) -> &str {
        "gap-fixture"
    }
    fn display_name(&self) -> &str {
        "Gap fixture"
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
        None
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
        let chunk =
            codeterm_plugin::jsonl_window::read_records_from(path, from, budget.max(1)).ok()?;
        let mut offset = from;
        let mut messages = Vec::new();
        for line in chunk.bytes.split_inclusive(|byte| *byte == b'\n') {
            let record: serde_json::Value = serde_json::from_slice(line).ok()?;
            messages.push(RichMessage {
                role: "user".into(),
                uuid: record["uuid"].as_str().map(str::to_owned),
                blocks: vec![MessageBlock {
                    kind: "text".into(),
                    data: serde_json::json!({"text": record["text"]}),
                }],
                record_start: Some(offset),
                ..Default::default()
            });
            offset += line.len() as u64;
        }
        Some(SessionDelta {
            new_offset: chunk.new_offset,
            messages,
        })
    }
}

fn texts(rows: &[ChatMsg]) -> Vec<String> {
    rows.iter()
        .filter(|row| row.system_subtype.as_deref() != Some(SUBTYPE))
        .map(|row| row.content.clone())
        .collect()
}

fn gap_offset(row: &ChatMsg) -> Option<u64> {
    match &row.timeline_marker {
        Some(ChatTimelineMarker::HistoryGap { before_offset, .. }) => Some(*before_offset),
        _ => None,
    }
}

/// Follows gap rows from `first` back to the start, oldest rows first.
fn page_back(
    svc: &ServiceContext,
    tab_id: &str,
    source: ChatHistorySource,
    session_id: &str,
    first: &[ChatMsg],
) -> Vec<String> {
    let mut pages = vec![texts(first)];
    let mut gap = first.iter().find_map(gap_offset);
    while let Some(before) = gap {
        let page = page(svc, tab_id, source, session_id, before).unwrap();
        assert!(page.iter().skip(1).all(|row| gap_offset(row).is_none()));
        gap = page.first().and_then(gap_offset);
        if let Some(next) = gap {
            assert!(next < before, "every page moves strictly older");
        }
        pages.push(texts(&page));
    }
    pages.into_iter().rev().flatten().collect()
}

#[test]
fn an_archived_session_past_the_window_starts_with_a_gap_that_pages_to_its_start() {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().to_string_lossy().into_owned();
    let pad = "p".repeat(4_000);
    let expected: Vec<String> = (0..600).map(|index| format!("row-{index:04}")).collect();
    let body: String = expected
        .iter()
        .map(|text| {
            format!(
                "{}\n",
                serde_json::json!({"uuid": text, "text": text, "pad": pad})
            )
        })
        .collect();
    std::fs::write(root.path().join("archived.jsonl"), body).unwrap();
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.cwd = Some(cwd.clone());
    tab.context_epoch_archives.push(ContextEpochArchive {
        provider: "gap-fixture".into(),
        cwd,
        session_id: "archived".into(),
        transport: ContextEpochTransport::AgentProvider,
        boundary: ContextEpochBoundary::ContextReset,
    });
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(PagedProvider));
    let archives = fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .unwrap()
        .context_epoch_archives
        .clone();

    let projected = super::super::session_segments::archived_provider_sessions(
        &fixture.service,
        &archives,
        "current",
    );
    assert!(projected[0].id.starts_with("provider-session:archived:"));
    assert!(
        gap_offset(&projected[0]).is_some(),
        "truncation is explicit"
    );
    let archived: Vec<ChatMsg> = projected
        .into_iter()
        .take_while(|row| row.system_subtype.as_deref() != Some("context_epoch_boundary"))
        .collect();
    let all = page_back(
        &fixture.service,
        &tab_id,
        ChatHistorySource::ProviderSession,
        "archived",
        &archived,
    );
    assert_eq!(
        all, expected,
        "every archived row is reachable exactly once"
    );

    assert!(page(
        &fixture.service,
        &tab_id,
        ChatHistorySource::ProviderSession,
        "foreign",
        10
    )
    .is_err());
    assert!(page(
        &fixture.service,
        &tab_id,
        ChatHistorySource::ChatBackend,
        "archived",
        10
    )
    .is_err());
}

#[test]
fn a_chat_backend_log_past_the_tail_pages_back_from_its_gap() {
    let plugin = "history-gap-fixture";
    let session = format!("gap-session-{}", std::process::id());
    let path = chatbackend_history::session_path(plugin, &session);
    let _ = std::fs::remove_file(&path);
    let pad = "q".repeat(8_000);
    let expected: Vec<String> = (0..700).map(|index| format!("msg-{index:04}")).collect();
    let mut history = ChatBackendHistory::open_at(path.clone());
    let rows: Vec<ChatMsg> = expected
        .iter()
        .map(|text| ChatMsg {
            id: text.clone(),
            kind: "assistant".into(),
            content: text.clone(),
            tool_result: Some(pad.clone()),
            ..Default::default()
        })
        .collect();
    history.upsert(&rows);
    let (mut mux, tab_id) = MultiplexerState::new();
    mux.get_tab_mut(&tab_id).unwrap().backing = TabBacking::ChatBackend {
        plugin_id: plugin.into(),
        session_id: session.clone(),
    };
    let fixture = ServiceFixture::new(mux);

    let tail = chatbackend_history::read_tail(&path).unwrap();
    assert!(tail.truncated());
    let first = bounded(
        ChatHistorySource::ChatBackend,
        &session,
        tail.start_offset,
        tail.messages,
    );
    let all = page_back(
        &fixture.service,
        &tab_id,
        ChatHistorySource::ChatBackend,
        &session,
        &first,
    );
    assert_eq!(all, expected);
    let _ = std::fs::remove_file(path);
}
