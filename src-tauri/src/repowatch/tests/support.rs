use crate::repowatch::backend::{ActiveWatch, BatchHandler, WatchBackend, WatchSignal, WatchSpec};
use crate::repowatch::events::{RepoEventSink, RepoWatchUnavailable};
use crate::repowatch::registry::RepoWatchRegistry;
use parking_lot::Mutex;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::Duration;

/// Bounded FAILURE deadline for a channel wait. A positive oracle fails when
/// it expires; a negative oracle passes when it expires.
pub const DEADLINE: Duration = Duration::from_secs(20);
/// Silence window for a negative oracle. Comfortably past the 200ms debounce
/// so a real event would already have been delivered.
pub const SILENCE: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SinkEvent {
    Changed(String),
    Unavailable(String, RepoWatchUnavailable),
}

pub struct ChannelSink {
    tx: Mutex<Sender<SinkEvent>>,
}

impl ChannelSink {
    pub fn new() -> (Arc<Self>, Receiver<SinkEvent>) {
        let (tx, rx) = channel();
        (Arc::new(Self { tx: Mutex::new(tx) }), rx)
    }
}

impl RepoEventSink for ChannelSink {
    fn repository_changed(&self, identity: &str) {
        let _ = self
            .tx
            .lock()
            .send(SinkEvent::Changed(identity.to_string()));
    }

    fn repository_unavailable(&self, identity: &str, reason: RepoWatchUnavailable) {
        let _ = self
            .tx
            .lock()
            .send(SinkEvent::Unavailable(identity.to_string(), reason));
    }
}

/// Waits on the channel for a change event naming `identity`, ignoring events
/// for other identities. Never sleeps; the deadline is a failure bound.
pub fn expect_change(rx: &Receiver<SinkEvent>, identity: &str, what: &str) {
    let deadline = std::time::Instant::now() + DEADLINE;
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        match rx.recv_timeout(left) {
            Ok(SinkEvent::Changed(got)) if same_path(&got, identity) => return,
            Ok(_) => continue,
            Err(RecvTimeoutError::Timeout) => {
                panic!("no repository-changed for {identity} after {what}")
            }
            Err(RecvTimeoutError::Disconnected) => {
                panic!("sink disconnected waiting for {what}")
            }
        }
    }
}

pub fn expect_unavailable(
    rx: &Receiver<SinkEvent>,
    reason: RepoWatchUnavailable,
    what: &str,
) -> String {
    let deadline = std::time::Instant::now() + DEADLINE;
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        match rx.recv_timeout(left) {
            Ok(SinkEvent::Unavailable(identity, got)) if got == reason => return identity,
            Ok(_) => continue,
            Err(_) => panic!("no {reason:?} unavailable signal after {what}"),
        }
    }
}

/// Every probe-driven dispatch is synchronous, so draining without a deadline
/// is exhaustive and fully deterministic.
pub fn collected(rx: &Receiver<SinkEvent>) -> Vec<SinkEvent> {
    let mut out = Vec::new();
    while let Ok(event) = rx.try_recv() {
        out.push(event);
    }
    out
}

/// Expiry of the bounded window IS the pass condition.
pub fn expect_silence(rx: &Receiver<SinkEvent>, what: &str) {
    let deadline = std::time::Instant::now() + SILENCE;
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return;
        }
        match rx.recv_timeout(left) {
            Ok(SinkEvent::Changed(id)) => panic!("{what} must emit no event, got change for {id}"),
            Ok(SinkEvent::Unavailable(id, reason)) => {
                panic!("{what} must emit no event, got {reason:?} for {id}")
            }
            Err(RecvTimeoutError::Timeout) => return,
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn same_path(a: &str, b: &str) -> bool {
    if cfg!(any(windows, target_os = "macos")) {
        a.to_lowercase() == b.to_lowercase()
    } else {
        a == b
    }
}

/// A backend that refuses every watch — the injection seam standing in for
/// permission denial, handle exhaustion, or a network filesystem.
pub struct RefusingBackend;

impl WatchBackend for RefusingBackend {
    fn build(
        &self,
        _specs: &[WatchSpec],
        _handler: BatchHandler,
    ) -> Result<Box<dyn ActiveWatch>, String> {
        Err("os refused the watch (injected)".to_string())
    }
}

struct InertWatch;
impl ActiveWatch for InertWatch {}

/// Accepts every watch but never delivers an event — isolates registry
/// bookkeeping from OS behaviour.
pub struct InertBackend;

impl WatchBackend for InertBackend {
    fn build(
        &self,
        _specs: &[WatchSpec],
        _handler: BatchHandler,
    ) -> Result<Box<dyn ActiveWatch>, String> {
        Ok(Box::new(InertWatch))
    }
}

/// Counts watcher construction and destruction so "one owning watcher per
/// identity" is observed rather than assumed.
#[derive(Default)]
pub struct WatchLedger {
    pub built: std::sync::atomic::AtomicUsize,
    pub dropped: std::sync::atomic::AtomicUsize,
}

impl WatchLedger {
    pub fn built(&self) -> usize {
        self.built.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn dropped(&self) -> usize {
        self.dropped.load(std::sync::atomic::Ordering::SeqCst)
    }
}

pub struct CountingBackend {
    pub ledger: Arc<WatchLedger>,
}

struct CountedWatch {
    ledger: Arc<WatchLedger>,
}

impl ActiveWatch for CountedWatch {}

impl Drop for CountedWatch {
    fn drop(&mut self) {
        self.ledger
            .dropped
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

impl WatchBackend for CountingBackend {
    fn build(
        &self,
        _specs: &[WatchSpec],
        _handler: BatchHandler,
    ) -> Result<Box<dyn ActiveWatch>, String> {
        self.ledger
            .built
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(Box::new(CountedWatch {
            ledger: self.ledger.clone(),
        }))
    }
}

/// Hands the handler back so a test can drive an exact batch or a runtime
/// backend failure with no filesystem involved.
#[derive(Default)]
pub struct ProbeBackend {
    pub taps: Arc<Mutex<Vec<Tap>>>,
}

pub struct Tap {
    pub specs: Vec<WatchSpec>,
    handler: Arc<Mutex<BatchHandler>>,
}

impl Tap {
    pub fn fire(&self, signal: WatchSignal) {
        let mut handler = self.handler.lock();
        (&mut **handler)(signal);
    }
}

impl ProbeBackend {
    pub fn paired() -> (Arc<Self>, Arc<Mutex<Vec<Tap>>>) {
        let backend = Arc::new(Self::default());
        let taps = backend.taps.clone();
        (backend, taps)
    }
}

/// The shared-ref watch is the one carrying the optional `refs/` spec.
pub fn fire_shared(taps: &Arc<Mutex<Vec<Tap>>>, signal: WatchSignal) -> usize {
    let taps = taps.lock();
    let mut fired = 0;
    for tap in taps.iter() {
        if tap.specs.iter().any(|s| s.optional) {
            tap.fire(signal.clone());
            fired += 1;
        }
    }
    fired
}

pub fn fire_worktree(taps: &Arc<Mutex<Vec<Tap>>>, signal: WatchSignal) -> usize {
    let taps = taps.lock();
    let mut fired = 0;
    for tap in taps.iter() {
        if !tap.specs.iter().any(|s| s.optional) {
            tap.fire(signal.clone());
            fired += 1;
        }
    }
    fired
}

impl WatchBackend for ProbeBackend {
    fn build(
        &self,
        specs: &[WatchSpec],
        handler: BatchHandler,
    ) -> Result<Box<dyn ActiveWatch>, String> {
        self.taps.lock().push(Tap {
            specs: specs.to_vec(),
            handler: Arc::new(Mutex::new(handler)),
        });
        Ok(Box::new(InertWatch))
    }
}

pub fn registry_with(
    backend: Arc<dyn WatchBackend>,
) -> (Arc<RepoWatchRegistry>, Receiver<SinkEvent>) {
    let (sink, rx) = ChannelSink::new();
    (Arc::new(RepoWatchRegistry::with_backend(sink, backend)), rx)
}

pub fn live_registry() -> (Arc<RepoWatchRegistry>, Receiver<SinkEvent>) {
    let (sink, rx) = ChannelSink::new();
    (Arc::new(RepoWatchRegistry::new(sink)), rx)
}

/// One registry per assertion: `subscribe` returns only once every backend
/// watch registered, and the channel starts empty. That is the barrier.
pub struct Watched {
    pub registry: Arc<RepoWatchRegistry>,
    pub rx: Receiver<SinkEvent>,
    pub identities: Vec<String>,
}

pub fn watch_all(roots: &[&Path]) -> Watched {
    let (registry, rx) = live_registry();
    let identities = roots
        .iter()
        .map(|root| {
            registry
                .subscribe(root)
                .unwrap_or_else(|e| panic!("subscribe {}: {e}", root.display()))
                .identity
        })
        .collect();
    Watched {
        registry,
        rx,
        identities,
    }
}

/// Watch first, act, then block on the channel for this action's own event.
pub fn expect_change_from(roots: &[&Path], target: usize, what: &str, action: impl FnOnce()) {
    let watched = watch_all(roots);
    action();
    expect_change(&watched.rx, &watched.identities[target], what);
}

pub fn git_available() -> bool {
    let mut command = std::process::Command::new("git");
    command.arg("--version");
    crate::process_util::no_window_std(&mut command);
    command
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn git(dir: &Path, args: &[&str]) {
    let mut command = std::process::Command::new("git");
    command
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Tester")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "Tester")
        .env("GIT_COMMITTER_EMAIL", "t@example.com");
    crate::process_util::no_window_std(&mut command);
    let out = command.output().expect("git run");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

pub fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

/// A primary repository with one commit, a `.gitignore` naming `ignored/`,
/// and a tracked file.
pub fn primary_repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "Tester"]);
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["config", "commit.gpgsign", "false"]);
    write(&root.join(".gitignore"), "ignored/\n");
    write(&root.join("tracked.txt"), "one\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "root"]);
    tmp
}

/// `git worktree add` — `.git` in the linked worktree is a FILE, which is the
/// normal case in this repository.
pub fn linked_worktree(primary: &Path, name: &str) -> tempfile::TempDir {
    let holder = tempfile::tempdir().unwrap();
    let target = holder.path().join(name);
    git(
        primary,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            name,
            target.to_str().unwrap(),
        ],
    );
    assert!(
        target.join(".git").is_file(),
        "linked worktree .git must be a FILE"
    );
    holder
}
