use super::*;
use crate::service::agent_runtime::model::{ItemCache, RuntimeState};
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::HashMap;

const THREAD: &str = "thread";

fn session() -> (Session, u64) {
    let mut state = RuntimeState::unloaded(Some(THREAD.into()));
    let epoch = state.begin();
    let session = Session {
        provider: "fixture".into(),
        adapter: Mutex::new(None),
        context: Mutex::new(RuntimeContext {
            epoch,
            tab_id: "tab".into(),
            binding_nonce: "nonce".into(),
            thread_id: Some(THREAD.into()),
            ..Default::default()
        }),
        controls_confirmation: Mutex::new(None),
        control_changes: tokio::sync::watch::channel(()).0,
        completions: Mutex::new(Default::default()),
        state: Mutex::new(state),
        cache: Mutex::new(ItemCache::default()),
        auth: Mutex::new(RuntimeAuthState::SignedIn),
        login: Mutex::new(Default::default()),
        thread_loaded: std::sync::atomic::AtomicBool::new(false),
        hook_state: Mutex::new(Value::Null),
        maps: Mutex::new(Default::default()),
        bookkeeping_failures: std::sync::atomic::AtomicU32::new(0),
        prompts: Mutex::new(HashMap::new()),
        process: Mutex::new(None),
        gate: tokio::sync::Semaphore::new(1),
        initialization: Default::default(),
        compaction: Mutex::new(Default::default()),
        abort_note: Mutex::new(None),
    };
    (session, epoch)
}

fn ready(session: &Session, epoch: u64) -> u64 {
    assert!(session.state.lock().load(epoch, THREAD));
    session.cache.lock().begin_backfill()
}

fn row(id: &str, content: &str) -> ChatMsg {
    ChatMsg {
        id: id.into(),
        content: content.into(),
        ..Default::default()
    }
}

fn page(ids: &[&str], next: Option<&str>) -> RuntimeResult {
    RuntimeResult::HistoryPage {
        thread_id: THREAD.into(),
        items: ids.iter().map(|id| row(id, "history")).collect(),
        next_cursor: next.map(str::to_string),
        open_items: Vec::new(),
        questions: None,
    }
}

#[derive(Default)]
struct FakePort {
    pages: HashMap<Option<String>, Result<RuntimeResult, RuntimeFailure>>,
    reads: Vec<Option<String>>,
    emitted: Vec<String>,
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
    async fn questions(&mut self, _: Vec<RuntimeHistoryQuestion>) -> Result<(), RuntimeFailure> {
        Ok(())
    }
    fn emit(&mut self, rows: &[ChatMsg]) {
        self.emitted.extend(rows.iter().map(|row| row.id.clone()));
    }
}

fn ids(session: &Session) -> Vec<String> {
    session
        .cache
        .lock()
        .snapshot()
        .into_iter()
        .map(|row| row.id)
        .collect()
}

fn assert_seq_ascending(session: &Session) {
    let rows = session.cache.lock().snapshot();
    assert!(rows.windows(2).all(|pair| pair[0].seq < pair[1].seq));
}

#[tokio::test]
async fn no_history_page_is_read_before_the_epoch_is_ready() {
    let (session, epoch) = session();
    let generation = session.cache.lock().begin_backfill();
    let mut port = FakePort::with(vec![(None, Ok(page(&["a"], None)))]);
    let outcome = replay(&session, epoch, generation, THREAD, &mut port).await;
    assert_eq!(outcome, Err(RuntimeFailure::Superseded));
    assert!(port.reads.is_empty());
    assert!(ids(&session).is_empty());

    let generation = ready(&session, epoch);
    let replayed = replay(&session, epoch, generation, THREAD, &mut port)
        .await
        .unwrap();
    assert_eq!(replayed.pages, 1);
    assert_eq!(ids(&session), ["a"]);
}

#[tokio::test]
async fn pages_land_newest_first_then_older_in_order_without_duplicates() {
    let (session, epoch) = session();
    let generation = ready(&session, epoch);
    let mut port = FakePort::with(vec![
        (None, Ok(page(&["e", "f"], Some("older")))),
        (Some("older"), Ok(page(&["c", "d", "e"], Some("oldest")))),
        (Some("oldest"), Ok(page(&["a", "b"], None))),
    ]);
    let replayed = replay(&session, epoch, generation, THREAD, &mut port)
        .await
        .unwrap();
    assert_eq!(replayed.pages, 3);
    assert_eq!(
        port.reads,
        [None, Some("older".into()), Some("oldest".into())]
    );
    assert_eq!(ids(&session), ["a", "b", "c", "d", "e", "f"]);
    assert_seq_ascending(&session);
    assert_eq!(&port.emitted[..2], ["e", "f"]);
}

#[tokio::test]
async fn a_send_during_backfill_stays_after_history_and_is_never_overridden() {
    let (session, epoch) = session();
    let generation = ready(&session, epoch);
    let mut bridge = LiveBridge {
        port: FakePort::with(vec![
            (None, Ok(page(&["c", "sent"], Some("older")))),
            (Some("older"), Ok(page(&["a", "b"], None))),
        ]),
        session: &session,
        ready_at_read: Vec::new(),
    };
    let replayed = replay(&session, epoch, generation, THREAD, &mut bridge)
        .await
        .unwrap();
    assert_eq!(replayed.pages, 2);
    assert_eq!(bridge.ready_at_read, [true, true]);
    assert_eq!(ids(&session), ["a", "b", "c", "sent"]);
    assert_seq_ascending(&session);
    let rows = session.cache.lock().snapshot();
    let sent: Vec<_> = rows.iter().filter(|row| row.id == "sent").collect();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].content, "live reply");
}

struct LiveBridge<'a> {
    port: FakePort,
    session: &'a Session,
    ready_at_read: Vec<bool>,
}

impl HistoryPort for LiveBridge<'_> {
    async fn read(&mut self, cursor: Option<String>) -> Result<RuntimeResult, RuntimeFailure> {
        self.ready_at_read.push(matches!(
            self.session.state.lock().phase,
            RuntimePhase::Ready
        ));
        let result = self.port.read(cursor).await;
        if self.ready_at_read.len() == 1 {
            self.session
                .cache
                .lock()
                .reconcile_live(row("sent", "live reply"), 1);
        }
        result
    }
    async fn questions(
        &mut self,
        questions: Vec<RuntimeHistoryQuestion>,
    ) -> Result<(), RuntimeFailure> {
        self.port.questions(questions).await
    }
    fn emit(&mut self, rows: &[ChatMsg]) {
        self.port.emit(rows);
    }
}

#[tokio::test]
async fn a_failing_page_stops_backfill_without_replaying_and_leaves_the_tab_ready() {
    let (session, epoch) = session();
    let generation = ready(&session, epoch);
    let mut port = FakePort::with(vec![
        (None, Ok(page(&["c"], Some("older")))),
        (
            Some("older"),
            Err(RuntimeFailure::HookFailed("page refused".into())),
        ),
    ]);
    let outcome = replay(&session, epoch, generation, THREAD, &mut port).await;
    assert_eq!(
        outcome,
        Err(RuntimeFailure::HookFailed("page refused".into()))
    );
    assert_eq!(port.reads, [None, Some("older".into())]);
    assert!(session
        .cache
        .lock()
        .finish_backfill(generation, HistoryBackfill::Failed));
    assert_eq!(session.cache.lock().backfill(), HistoryBackfill::Failed);
    assert_eq!(ids(&session), ["c"]);
    assert!(matches!(session.state.lock().phase, RuntimePhase::Ready));
}

#[tokio::test]
async fn a_superseded_epoch_stops_backfill_between_pages() {
    let (session, epoch) = session();
    let generation = ready(&session, epoch);
    let mut port = FakePort::with(vec![
        (None, Ok(page(&["c"], Some("older")))),
        (Some("older"), Ok(page(&["a"], None))),
    ]);
    let newer = session.cache.lock().begin_backfill();
    assert_ne!(newer, generation);
    let outcome = replay(&session, epoch, generation, THREAD, &mut port).await;
    assert_eq!(outcome, Err(RuntimeFailure::Superseded));
    assert!(port.reads.is_empty());
    assert!(!session
        .cache
        .lock()
        .finish_backfill(generation, HistoryBackfill::Complete));
}

#[tokio::test]
async fn repeated_cursor_is_refused() {
    let (session, epoch) = session();
    let generation = ready(&session, epoch);
    let mut port = FakePort::with(vec![
        (None, Ok(page(&["b"], Some("loop")))),
        (Some("loop"), Ok(page(&["a"], Some("loop")))),
    ]);
    let outcome = replay(&session, epoch, generation, THREAD, &mut port).await;
    assert_eq!(
        outcome,
        Err(RuntimeFailure::HookFailed("History cursor limit".into()))
    );
    assert_eq!(ids(&session), ["a", "b"]);
}
