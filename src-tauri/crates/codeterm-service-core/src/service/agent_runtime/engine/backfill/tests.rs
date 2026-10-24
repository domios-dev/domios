use super::*;
use crate::service::agent_runtime::model::{ItemCache, RuntimeState};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashMap;

const THREAD: &str = "thread";

fn session() -> (Session, u64) {
    let mut state = RuntimeState::unloaded(Some(THREAD.into()));
    let epoch = state.begin();
    let session = Session {
        update_generation: std::sync::atomic::AtomicU64::new(0),
        update_recovery_available: std::sync::atomic::AtomicBool::new(false),
        provider: "fixture".into(),
        adapter: Mutex::new(None),
        context: Mutex::new(RuntimeContext {
            host_owned: Default::default(),
            epoch,
            tab_id: "tab".into(),
            binding_nonce: "nonce".into(),
            thread_id: Some(THREAD.into()),
            ..Default::default()
        }),
        controls_confirmation: Mutex::new(None),
        control_changes: tokio::sync::watch::channel(()).0,
        reverted_thread: Mutex::new(None),
        completions: Mutex::new(Default::default()),
        state: Mutex::new(state),
        cache: Mutex::new(ItemCache::default()),
        auth: Mutex::new(RuntimeAuthState::SignedIn),
        login: Mutex::new(Default::default()),
        thread_loaded: std::sync::atomic::AtomicBool::new(false),
        hook_state: Mutex::new(Value::Null),
        maps: Mutex::new(Default::default()),
        provider_operation_failures: std::sync::atomic::AtomicU32::new(0),
        bookkeeping_warning_emitted: std::sync::atomic::AtomicBool::new(false),
        prompts: Mutex::new(HashMap::new()),
        process: crate::service::agent_runtime::process_slot::ProcessSlot::new(None),
        gate: tokio::sync::Semaphore::new(1),
        initialization: Default::default(),
        compaction: Mutex::new(Default::default()),
        abort_note: Mutex::new(None),
        history_retry: Mutex::new(None),
        capacity_retry: Mutex::new(Default::default()),
    };
    (session, epoch)
}

fn replay_for(session: &Session, epoch: u64) -> Replay {
    let generation = session.cache.lock().begin_backfill();
    Replay::new(epoch, generation, THREAD)
}

fn make_ready(session: &Session, epoch: u64) {
    assert!(session.state.lock().load(epoch, THREAD));
}

fn row(id: &str, content: &str) -> ChatMsg {
    ChatMsg {
        id: id.into(),
        content: content.into(),
        ..Default::default()
    }
}

fn page_in(
    ids: &[&str],
    next: Option<&str>,
    order: Option<HistoryOrder>,
) -> Result<RuntimeResult, RuntimeFailure> {
    Ok(RuntimeResult::HistoryPage {
        thread_id: THREAD.into(),
        items: ids.iter().map(|id| row(id, "history")).collect(),
        next_cursor: next.map(str::to_string),
        open_items: Vec::new(),
        questions: None,
        order,
    })
}

fn page(ids: &[&str], next: Option<&str>) -> Result<RuntimeResult, RuntimeFailure> {
    page_in(ids, next, Some(HistoryOrder::Desc))
}

#[derive(Default)]
struct FakePort {
    pages: HashMap<Option<String>, Result<RuntimeResult, RuntimeFailure>>,
    reads: Vec<Option<String>>,
    questions: Vec<Vec<RuntimeHistoryQuestion>>,
    states: Vec<RuntimeTurnState>,
}

impl FakePort {
    fn with(pages: Vec<(Option<&str>, Result<RuntimeResult, RuntimeFailure>)>) -> Self {
        Self {
            pages: pages
                .into_iter()
                .map(|(cursor, page)| (cursor.map(str::to_string), page))
                .collect(),
            ..Default::default()
        }
    }
}

impl HistoryPort for FakePort {
    async fn read(&mut self, cursor: Option<String>) -> Result<RuntimeResult, RuntimeFailure> {
        self.reads.push(cursor.clone());
        self.pages
            .get(&cursor)
            .cloned()
            .unwrap_or_else(|| Err(RuntimeFailure::HookFailed("unexpected cursor".into())))
    }
    async fn questions(
        &mut self,
        questions: Vec<RuntimeHistoryQuestion>,
    ) -> Result<(), RuntimeFailure> {
        self.questions.push(questions);
        Ok(())
    }
    fn restored_state(&mut self, state: RuntimeTurnState) {
        self.states.push(state);
    }
}

fn rows(session: &Session) -> Vec<(String, i64)> {
    session
        .cache
        .lock()
        .snapshot()
        .into_iter()
        .map(|row| (row.id, row.seq))
        .collect()
}

fn ids(session: &Session) -> Vec<String> {
    rows(session).into_iter().map(|(id, _)| id).collect()
}

fn assert_seq_ascending(session: &Session) {
    let rows = rows(session);
    assert!(rows.windows(2).all(|pair| pair[0].1 < pair[1].1));
}

#[tokio::test]
async fn exactly_one_page_lands_before_ready_and_older_pages_wait_for_ready() {
    let (session, epoch) = session();
    let mut replay = replay_for(&session, epoch);
    let mut port = FakePort::with(vec![
        (None, page(&["c"], Some("older"))),
        (Some("older"), page(&["a", "b"], None)),
    ]);
    replay.first(&session, &mut port).await.unwrap();
    assert_eq!(port.reads, [None]);
    assert!(!replay.done);
    assert_eq!(
        replay.rest(&session, &mut port).await,
        Err(RuntimeFailure::Superseded)
    );
    assert_eq!(port.reads, [None]);

    make_ready(&session, epoch);
    replay.rest(&session, &mut port).await.unwrap();
    assert_eq!(port.reads, [None, Some("older".into())]);
    assert_eq!(ids(&session), ["a", "b", "c"]);
}

#[tokio::test]
async fn the_first_page_adopts_a_running_restored_turn_before_ready() {
    let (session, epoch) = session();
    session.completions.lock().record(THREAD, "finished");
    let mut replay = replay_for(&session, epoch);
    let open = |turn: &str, id: &str| OpenItem {
        turn_id: turn.into(),
        item: row(id, "running"),
    };
    let mut port = FakePort::with(vec![(
        None,
        Ok(RuntimeResult::HistoryPage {
            thread_id: THREAD.into(),
            items: vec![row("u", "question")],
            next_cursor: Some("older".into()),
            open_items: vec![open("finished", "x"), open("running", "y")],
            questions: None,
            order: Some(HistoryOrder::Desc),
        }),
    )]);
    replay.first(&session, &mut port).await.unwrap();
    assert!(matches!(session.state.lock().phase, RuntimePhase::Loading));
    assert_eq!(
        session.context.lock().active_turn_id.as_deref(),
        Some("running")
    );
    assert_eq!(port.states, [RuntimeTurnState::Working]);
}

#[tokio::test]
async fn replay_cannot_restore_a_finished_turn_as_running() {
    let (session, epoch) = session();
    session.completions.lock().record(THREAD, "finished");
    let mut replay = replay_for(&session, epoch);
    let command = ChatMsg {
        id: "finished-command".into(),
        kind: "tool_call".into(),
        tool_flow_id: Some("finished".into()),
        tool_status: Some("running".into()),
        ..Default::default()
    };
    let mut port = FakePort::with(vec![(
        None,
        Ok(RuntimeResult::HistoryPage {
            thread_id: THREAD.into(),
            items: vec![command.clone()],
            open_items: vec![OpenItem {
                turn_id: "finished".into(),
                item: command,
            }],
            next_cursor: None,
            questions: None,
            order: Some(HistoryOrder::Desc),
        }),
    )]);
    replay.first(&session, &mut port).await.unwrap();
    assert!(session.context.lock().active_turn_id.is_none());
    assert!(!port.states.contains(&RuntimeTurnState::Working));
    assert_eq!(
        session.cache.lock().snapshot()[0].tool_status.as_deref(),
        Some("interrupted")
    );
}

#[tokio::test]
async fn restored_status_uses_only_the_newest_page_of_the_bound_thread() {
    for foreign in [false, true] {
        let (session, epoch) = session();
        let mut replay = replay_for(&session, epoch);
        let mut answer = row("answer", "opaque");
        answer.kind = "assistant".into();
        let mut port = FakePort::with(vec![
            (
                None,
                Ok(RuntimeResult::HistoryPage {
                    thread_id: if foreign { "foreign" } else { THREAD }.into(),
                    items: vec![answer],
                    next_cursor: Some("older".into()),
                    open_items: vec![],
                    questions: None,
                    order: Some(HistoryOrder::Desc),
                }),
            ),
            (Some("older"), page(&["user"], None)),
        ]);
        let first = replay.first(&session, &mut port).await;
        if foreign {
            assert_eq!(first, Err(RuntimeFailure::Superseded));
            assert!(port.states.is_empty());
        } else {
            first.unwrap();
            assert_eq!(port.states, [RuntimeTurnState::Idle]);
            make_ready(&session, epoch);
            replay.rest(&session, &mut port).await.unwrap();
            assert_eq!(port.states, [RuntimeTurnState::Idle]);
        }
    }
}

#[tokio::test]
async fn older_pages_take_lower_seqs_without_renumbering_or_duplicates() {
    let (session, epoch) = session();
    let mut replay = replay_for(&session, epoch);
    let mut port = FakePort::with(vec![
        (None, page(&["e", "f"], Some("older"))),
        (Some("older"), page(&["c", "d", "e"], Some("oldest"))),
        (Some("oldest"), page(&["a", "b"], None)),
    ]);
    replay.first(&session, &mut port).await.unwrap();
    let newest = rows(&session);
    make_ready(&session, epoch);
    replay.rest(&session, &mut port).await.unwrap();
    assert_eq!(replay.pages, 3);
    assert_eq!(ids(&session), ["a", "b", "c", "d", "e", "f"]);
    assert_seq_ascending(&session);
    let after = rows(&session);
    for row in &newest {
        assert!(after.contains(row));
    }
    assert_eq!(session.cache.lock().history_pages(), 3);
}

#[tokio::test]
async fn a_send_during_backfill_keeps_its_seq_and_is_never_overridden() {
    let (session, epoch) = session();
    let mut replay = replay_for(&session, epoch);
    let mut port = FakePort::with(vec![
        (None, page(&["c"], Some("older"))),
        (Some("older"), page(&["a", "b", "c", "sent"], None)),
    ]);
    replay.first(&session, &mut port).await.unwrap();
    make_ready(&session, epoch);
    let sent = session
        .cache
        .lock()
        .reconcile_live(row("sent", "live reply"), 1);
    session
        .cache
        .lock()
        .reconcile_live(row("c", "live revision"), 2);
    replay.rest(&session, &mut port).await.unwrap();
    assert_eq!(ids(&session), ["a", "b", "c", "sent"]);
    assert_seq_ascending(&session);
    let cached = session.cache.lock().snapshot();
    let only = |id: &str| -> Vec<&ChatMsg> { cached.iter().filter(|row| row.id == id).collect() };
    assert_eq!(only("sent").len(), 1);
    assert_eq!(only("sent")[0].content, "live reply");
    assert_eq!(only("sent")[0].seq, sent.seq);
    assert_eq!(only("c")[0].content, "live revision");
}

#[tokio::test]
async fn a_failing_page_stops_backfill_without_replaying_and_leaves_the_tab_ready() {
    let (session, epoch) = session();
    let mut replay = replay_for(&session, epoch);
    let mut port = FakePort::with(vec![
        (None, page(&["c"], Some("older"))),
        (
            Some("older"),
            Err(RuntimeFailure::HookFailed("page refused".into())),
        ),
    ]);
    replay.first(&session, &mut port).await.unwrap();
    make_ready(&session, epoch);
    assert_eq!(
        replay.rest(&session, &mut port).await,
        Err(RuntimeFailure::HookFailed("page refused".into()))
    );
    assert_eq!(port.reads, [None, Some("older".into())]);
    assert!(session
        .cache
        .lock()
        .finish_backfill(replay.generation, HistoryBackfill::Failed));
    assert_eq!(session.cache.lock().backfill(), HistoryBackfill::Failed);
    assert_eq!(ids(&session), ["c"]);
    assert!(matches!(session.state.lock().phase, RuntimePhase::Ready));
}

#[tokio::test]
async fn a_newer_generation_stops_backfill_between_pages() {
    let (session, epoch) = session();
    let mut replay = replay_for(&session, epoch);
    let mut port = FakePort::with(vec![
        (None, page(&["c"], Some("older"))),
        (Some("older"), page(&["a"], None)),
    ]);
    replay.first(&session, &mut port).await.unwrap();
    make_ready(&session, epoch);
    session.cache.lock().begin_backfill();
    assert_eq!(
        replay.rest(&session, &mut port).await,
        Err(RuntimeFailure::Superseded)
    );
    assert_eq!(port.reads, [None]);
    assert!(!session
        .cache
        .lock()
        .finish_backfill(replay.generation, HistoryBackfill::Complete));
}

#[tokio::test]
async fn repeated_cursor_is_refused() {
    let (session, epoch) = session();
    let mut replay = replay_for(&session, epoch);
    let mut port = FakePort::with(vec![
        (None, page(&["b"], Some("loop"))),
        (Some("loop"), page(&["a"], Some("loop"))),
    ]);
    replay.first(&session, &mut port).await.unwrap();
    make_ready(&session, epoch);
    assert_eq!(
        replay.rest(&session, &mut port).await,
        Err(RuntimeFailure::HookFailed(
            "History cursor did not advance".into()
        ))
    );
    assert_eq!(ids(&session), ["b"]);
}

#[tokio::test]
async fn a_plugin_answering_ascending_is_replayed_whole_before_ready_below_kept_rows() {
    let (session, epoch) = session();
    session
        .cache
        .lock()
        .reconcile_live(row("host", "warning"), 1);
    let mut replay = replay_for(&session, epoch);
    let mut port = FakePort::with(vec![
        (None, page_in(&["a", "b"], Some("newer"), None)),
        (Some("newer"), page_in(&["c"], None, None)),
    ]);
    replay.first(&session, &mut port).await.unwrap();
    assert!(replay.done);
    assert!(!replay.descending);
    assert_eq!(port.reads, [None, Some("newer".into())]);
    assert_eq!(ids(&session), ["a", "b", "c", "host"]);
    assert_seq_ascending(&session);
    assert_eq!(session.cache.lock().history_pages(), 2);
}

#[tokio::test]
async fn a_new_replay_drops_previous_history_rows_and_lands_below_kept_live_rows() {
    let (session, epoch) = session();
    let mut replay = replay_for(&session, epoch);
    let mut port = FakePort::with(vec![(None, page(&["a", "b"], None))]);
    replay.first(&session, &mut port).await.unwrap();
    assert!(replay.done);
    assert!(session
        .cache
        .lock()
        .finish_backfill(replay.generation, HistoryBackfill::Complete));
    session
        .cache
        .lock()
        .reconcile_live(row("aborted", "turn aborted"), 1);
    assert_eq!(ids(&session), ["a", "b", "aborted"]);

    let mut again = replay_for(&session, epoch);
    assert_eq!(ids(&session), ["aborted"]);
    let mut port = FakePort::with(vec![
        (None, page(&["b", "c"], Some("older"))),
        (Some("older"), page(&["a"], None)),
    ]);
    again.first(&session, &mut port).await.unwrap();
    assert_eq!(ids(&session), ["b", "c", "aborted"]);
    make_ready(&session, epoch);
    again.rest(&session, &mut port).await.unwrap();
    assert_eq!(ids(&session), ["a", "b", "c", "aborted"]);
    assert_seq_ascending(&session);
}

#[tokio::test]
async fn a_question_on_an_older_page_resolves_from_an_answer_on_a_newer_page() {
    let (session, epoch) = session();
    let mut replay = replay_for(&session, epoch);
    let question = RuntimeHistoryQuestion {
        thread_id: THREAD.into(),
        item_id: "ask".into(),
        questions: vec![RuntimeQuestion {
            id: "color".into(),
            text: "Choose color".into(),
            options: vec![codeterm_types::pty::PromptOption {
                label: "red".into(),
                value: "red".into(),
            }],
            multi_select: None,
            allow_free_text: None,
        }],
        response: RuntimeQuestionResponse::Message,
        resolved: None,
    };
    let mut answer = row("answer", "");
    answer.kind = "user".into();
    answer.content =
        json!({"itemId":"ask","answers":[{"questionId":"color","values":["red"]}]}).to_string();
    let mut port = FakePort::with(vec![
        (
            None,
            Ok(RuntimeResult::HistoryPage {
                thread_id: THREAD.into(),
                items: vec![answer],
                next_cursor: Some("older".into()),
                open_items: Vec::new(),
                questions: None,
                order: Some(HistoryOrder::Desc),
            }),
        ),
        (
            Some("older"),
            Ok(RuntimeResult::HistoryPage {
                thread_id: THREAD.into(),
                items: vec![row("ask", "Choose color")],
                next_cursor: None,
                open_items: Vec::new(),
                questions: Some(vec![question]),
                order: Some(HistoryOrder::Desc),
            }),
        ),
    ]);
    replay.first(&session, &mut port).await.unwrap();
    make_ready(&session, epoch);
    replay.rest(&session, &mut port).await.unwrap();
    let last = port.questions.last().unwrap();
    assert_eq!(last.len(), 1);
    assert_eq!(last[0].resolved, Some(true));
}

#[tokio::test]
async fn failed_history_checkpoint_retries_same_page_without_clearing_landed_rows() {
    let (session, epoch) = session();
    let mut replay = replay_for(&session, epoch);
    let mut port = FakePort::with(vec![
        (None, page(&["c"], Some("older"))),
        (Some("older"), Err(RuntimeFailure::Deadline)),
    ]);
    replay.first(&session, &mut port).await.unwrap();
    make_ready(&session, epoch);
    assert_eq!(
        replay.rest(&session, &mut port).await,
        Err(RuntimeFailure::Deadline)
    );
    assert!(session
        .cache
        .lock()
        .finish_backfill(replay.generation, HistoryBackfill::Failed));
    session
        .cache
        .lock()
        .reconcile_live(row("c", "live revision"), 2);
    let checkpoint = FailedReplay {
        replay: replay.clone(),
        error: "fixture deadline".into(),
    };
    let mut replay = checkpoint.resume(&session).unwrap();
    assert!(!session.cache.lock().resume_backfill(replay.generation));
    let mut retry = FakePort::with(vec![(Some("older"), page(&["a", "b", "c"], None))]);
    replay.rest(&session, &mut retry).await.unwrap();
    assert_eq!(retry.reads, [Some("older".into())]);
    assert_eq!(ids(&session), ["a", "b", "c"]);
    assert_eq!(
        session.cache.lock().snapshot().last().unwrap().content,
        "live revision"
    );
    assert_seq_ascending(&session);
    session.context.lock().epoch += 1;
    assert!(!replay.current(&session, true));
}

#[tokio::test]
async fn failed_checkpoint_refuses_a_new_binding_epoch_without_changing_cache() {
    let (session, epoch) = session();
    let replay = replay_for(&session, epoch);
    make_ready(&session, epoch);
    session
        .cache
        .lock()
        .finish_backfill(replay.generation, HistoryBackfill::Failed);
    let checkpoint = FailedReplay {
        replay,
        error: "fixture".into(),
    };
    session.context.lock().epoch += 1;
    assert!(matches!(
        checkpoint.resume(&session),
        Err(RuntimeFailure::Superseded)
    ));
    assert_eq!(session.cache.lock().backfill(), HistoryBackfill::Failed);
}

#[test]
fn history_failure_snapshot_keeps_runtime_loaded_with_precise_page_error() {
    let (session, epoch) = session();
    let replay = replay_for(&session, epoch);
    make_ready(&session, epoch);
    for _ in 0..8 {
        session.cache.lock().note_history_page();
    }
    session
        .cache
        .lock()
        .finish_backfill(replay.generation, HistoryBackfill::Failed);
    let error = "History page 9: fixture execution deadline".to_string();
    *session.history_retry.lock() = Some(FailedReplay {
        replay,
        error: error.clone(),
    });
    let snapshot = serde_json::to_value(super::super::snapshot(&session)).unwrap();
    assert_eq!(snapshot["state"], "loaded");
    assert_eq!(snapshot["history"], "failed");
    assert_eq!(snapshot["historyPages"], 8);
    assert_eq!(snapshot["historyError"], error);
}

#[tokio::test]
async fn logical_500mb_restore_stops_at_two_pages_and_preserves_older_cursor() {
    struct LogicalHistory {
        reads: usize,
    }
    impl HistoryPort for LogicalHistory {
        async fn read(&mut self, cursor: Option<String>) -> Result<RuntimeResult, RuntimeFailure> {
            let page_number: usize = cursor.as_deref().unwrap_or("500").parse().unwrap();
            self.reads += 1;
            Ok(RuntimeResult::HistoryPage {
                thread_id: THREAD.into(),
                items: vec![row(&page_number.to_string(), &"x".repeat(1024 * 1024))],
                next_cursor: (page_number > 1).then(|| (page_number - 1).to_string()),
                open_items: vec![],
                questions: None,
                order: Some(HistoryOrder::Desc),
            })
        }
        async fn questions(
            &mut self,
            _: Vec<RuntimeHistoryQuestion>,
        ) -> Result<(), RuntimeFailure> {
            Ok(())
        }
    }
    let (session, epoch) = session();
    session.cache.lock().bound_rereadable();
    let mut replay = replay_for(&session, epoch);
    replay.bound_startup();
    let mut port = LogicalHistory { reads: 0 };
    replay.first(&session, &mut port).await.unwrap();
    make_ready(&session, epoch);
    replay.rest(&session, &mut port).await.unwrap();
    assert_eq!(port.reads, STARTUP_HISTORY_PAGES as usize);
    assert_eq!(replay.cursor.as_deref(), Some("498"));
    assert!(!replay.done);
    assert!(
        session.cache.lock().retained().1
            <= crate::service::agent_runtime::model::REREADABLE_CACHE_BYTES
    );
    assert!(session.cache.lock().snapshot().len() <= 2);
    replay.rest(&session, &mut port).await.unwrap();
    assert_eq!(port.reads, STARTUP_HISTORY_PAGES as usize);
    let mut cursor = replay.cursor.clone();
    let mut oldest = String::new();
    while cursor.is_some() {
        let RuntimeResult::HistoryPage {
            items, next_cursor, ..
        } = port.read(cursor).await.unwrap()
        else {
            panic!("expected page")
        };
        oldest = items[0].id.clone();
        cursor = next_cursor;
        assert!(session.cache.lock().snapshot().len() <= 2);
    }
    assert_eq!(oldest, "1");
    assert_eq!(port.reads, 500);
}

#[tokio::test]
async fn history_concurrency_admits_only_two_restored_tabs_until_a_permit_releases() {
    let budget = &*HISTORY_READS;
    let first = budget.try_acquire().unwrap();
    let second = budget.try_acquire().unwrap();
    let mut queued = Vec::new();
    for _ in 0..20 {
        let mut pending = Box::pin(budget.acquire());
        assert!(
            std::future::poll_fn(|cx| std::task::Poll::Ready(pending.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        queued.push(pending);
    }
    drop(first);
    let admitted = queued.remove(0).await.unwrap();
    assert!(budget.try_acquire().is_err());
    drop(second);
    drop(admitted);
    for pending in queued {
        drop(pending.await.unwrap());
    }
    assert_eq!(budget.available_permits(), HISTORY_CONCURRENCY);
}
