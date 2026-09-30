use portable_pty::{native_pty_system, CommandBuilder, PtyPair, PtySize};
use std::collections::HashMap;
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::Instant;

use parking_lot::Mutex as PlMutex;

pub use super::child_exit::ChildExit;
pub use super::spawn_gate::PtySpawnGate;
pub use super::teardown::{PtyReaper, PtyTeardownSignal};
use super::write_queue::{GuardedInputGate, GuardedInputPermit, WriteQueue};

pub struct SpawnMark {
    pub when: Instant,
    pub shell: String,
}

fn spawn_times() -> &'static PlMutex<HashMap<String, SpawnMark>> {
    static SPAWN_TIMES: OnceLock<PlMutex<HashMap<String, SpawnMark>>> = OnceLock::new();
    SPAWN_TIMES.get_or_init(|| PlMutex::new(HashMap::new()))
}

fn next_session_generation() -> u64 {
    static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);
    NEXT_GENERATION.fetch_add(1, Ordering::Relaxed)
}

pub fn record_spawn_time(pty_id: &str, shell: &str) {
    spawn_times().lock().insert(
        pty_id.to_string(),
        SpawnMark {
            when: Instant::now(),
            shell: shell.to_string(),
        },
    );
}

pub fn take_spawn_time(pty_id: &str) -> Option<SpawnMark> {
    spawn_times().lock().remove(pty_id)
}

/// A PTY session — stores the pair, reader, writer, child.
pub struct PtySession {
    pub generation: u64,
    pub pair: StdMutex<Option<PtyPair>>,
    pub size: StdMutex<(u16, u16)>,
    pub reader: StdMutex<Option<Box<dyn Read + Send>>>,
    pub write_queue: WriteQueue,
    pub guarded_input: GuardedInputGate,
    pub child: StdMutex<Box<dyn portable_pty::Child + Send + Sync>>,
    /// Backend reader channel (bounded). When Some, a backend reader is active and sending bytes here.
    /// read_pty reads from this instead of the raw PTY reader to avoid competing reads.
    /// Bounded to prevent memory leaks when frontend isn't consuming (hidden tabs).
    pub backend_reader_tx: StdMutex<Option<tokio::sync::mpsc::Sender<Vec<u8>>>>,
    pub backend_reader_rx: StdMutex<Option<tokio::sync::mpsc::Receiver<Vec<u8>>>>,
    pub backend_reader_active: AtomicBool,
    pub teardown: PtyTeardownSignal,
    /// Recorded once when the child process exits, with its exit code.
    pub child_exit: Arc<ChildExit>,
}

impl Drop for PtySession {
    fn drop(&mut self) {
        // Covers drops that bypass observe_exit, e.g. a panic unwinding through
        // an unprocessed reaper; closing inline can block (see teardown.rs).
        if let Ok(mut pair) = self.pair.lock() {
            if let Some(pair) = pair.take() {
                std::thread::spawn(move || drop(pair));
            }
        }
    }
}

impl PtySession {
    /// The child's exit code once it has exited and one was observable.
    pub fn child_exit_code(&self) -> Option<i32> {
        self.child_exit.code()
    }

    pub fn write_and_flush_guarded(
        &self,
        data: Vec<u8>,
        permit: GuardedInputPermit,
        timeout: std::time::Duration,
    ) -> Result<(), String> {
        self.write_queue
            .send_and_wait_guarded(data, permit, timeout)
    }

    pub fn write_frames_and_flush_guarded(
        &self,
        frames: Vec<Vec<u8>>,
        permit: GuardedInputPermit,
        timeout: std::time::Duration,
    ) -> Result<(), String> {
        self.write_queue
            .send_frames_and_wait_guarded(frames, permit, timeout)
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<bool, String> {
        let mut size = self.size.lock().map_err(|e| e.to_string())?;
        if *size == (cols, rows) {
            return Ok(false);
        }
        let pair = self.pair.lock().map_err(|e| e.to_string())?;
        let pair = pair
            .as_ref()
            .ok_or_else(|| "PTY already closed".to_string())?;
        pair.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("Resize failed: {}", e))?;
        *size = (cols, rows);
        Ok(true)
    }

    pub fn mark_backend_reader_started(&self) {
        self.backend_reader_active.store(true, Ordering::Release);
        self.teardown.mark_reader_started();
    }

    pub fn mark_backend_reader_finished(&self) {
        self.backend_reader_active.store(false, Ordering::Release);
        self.teardown.mark_reader_finished();
    }
}

/// Ring buffer storing last N bytes of PTY output per tab.
/// Also maintains a full history (capped at MAX_FULL_SIZE) for chat view parsing.
///
/// Screen parsing (VT100) lives in `ScreenState` behind a separate lock
/// to avoid contention between output processing and chat view reads.
pub struct OutputBuffer {
    buffers: HashMap<String, Vec<u8>>,
    /// Full output history (capped at MAX_FULL_SIZE per PTY). Used by chat view parser.
    full_buffers: HashMap<String, Vec<u8>>,
    /// Monotonic logical output byte count per PTY, including seeded history.
    /// SSE clients use this as a stable absolute offset.
    total_written: HashMap<String, u64>,
    max_size: usize,
    max_full: usize,
}

/// Maximum size of full_buffers per PTY (2 MB).
const MAX_FULL_SIZE: usize = 2 * 1024 * 1024;

impl OutputBuffer {
    pub fn new(max_size: usize) -> Self {
        Self::new_with_caps(max_size, MAX_FULL_SIZE)
    }

    fn new_with_caps(ring_size: usize, full_size: usize) -> Self {
        Self {
            buffers: HashMap::new(),
            full_buffers: HashMap::new(),
            total_written: HashMap::new(),
            max_size: ring_size,
            max_full: full_size,
        }
    }

    pub fn append(&mut self, pty_id: &str, data: &[u8]) {
        // Ring buffer (truncated)
        let buf = self.buffers.entry(pty_id.to_string()).or_default();
        buf.extend_from_slice(data);
        if buf.len() > self.max_size {
            let drain = buf.len() - self.max_size;
            buf.drain(..drain);
        }
        // Full history (capped at max_full)
        let full = self.full_buffers.entry(pty_id.to_string()).or_default();
        full.extend_from_slice(data);
        if full.len() > self.max_full {
            let excess = full.len() - self.max_full;
            full.drain(..excess);
        }
        *self.total_written.entry(pty_id.to_string()).or_insert(0) += data.len() as u64;
    }

    pub fn prepend(&mut self, pty_id: &str, data: &[u8]) {
        if data.is_empty() {
            return;
        }

        let buf = self.buffers.entry(pty_id.to_string()).or_default();
        prepend_capped(buf, data, self.max_size);

        let full = self.full_buffers.entry(pty_id.to_string()).or_default();
        prepend_capped(full, data, self.max_full);

        *self.total_written.entry(pty_id.to_string()).or_insert(0) += data.len() as u64;
    }

    pub fn get(&self, pty_id: &str) -> Option<&[u8]> {
        self.buffers.get(pty_id).map(|v| v.as_slice())
    }

    /// Get full untruncated output history.
    pub fn get_full(&self, pty_id: &str) -> Option<&[u8]> {
        self.full_buffers.get(pty_id).map(|v| v.as_slice())
    }

    pub fn get_text(&self, pty_id: &str) -> Option<String> {
        self.get(pty_id)
            .map(|b| String::from_utf8_lossy(b).to_string())
    }

    /// Get full untruncated output as text.
    pub fn get_full_text(&self, pty_id: &str) -> Option<String> {
        self.get_full(pty_id)
            .map(|b| String::from_utf8_lossy(b).to_string())
    }

    /// Total logical output bytes represented for this PTY (monotonic).
    pub fn total_written(&self, pty_id: &str) -> u64 {
        self.total_written.get(pty_id).copied().unwrap_or(0)
    }

    /// Read everything written after `offset`. If `offset` predates the
    /// retained full_buffer window (rotated out), starts from the oldest
    /// retained byte. Returns (data, new_total) — caller advances its
    /// offset to `new_total` regardless of how much data was returned.
    pub fn read_since(&self, pty_id: &str, offset: u64) -> (Vec<u8>, u64) {
        let total = self.total_written(pty_id);
        let buf = match self.full_buffers.get(pty_id) {
            Some(b) => b,
            None => return (Vec::new(), total),
        };
        if offset >= total {
            return (Vec::new(), total);
        }
        let buf_start = total.saturating_sub(buf.len() as u64);
        let read_from = offset.max(buf_start);
        let local_idx = (read_from - buf_start) as usize;
        let slice = buf.get(local_idx..).map(|s| s.to_vec()).unwrap_or_default();
        (slice, total)
    }

    pub fn remove(&mut self, pty_id: &str) {
        self.buffers.remove(pty_id);
        self.full_buffers.remove(pty_id);
        self.total_written.remove(pty_id);
    }
}

fn prepend_capped(buffer: &mut Vec<u8>, prefix: &[u8], max_size: usize) {
    let mut combined = Vec::with_capacity(prefix.len().saturating_add(buffer.len()));
    combined.extend_from_slice(prefix);
    combined.extend_from_slice(buffer);
    if combined.len() > max_size {
        combined.drain(..combined.len() - max_size);
    }
    *buffer = combined;
}

#[cfg(test)]
mod output_buffer_tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Instant;

    #[test]
    fn read_since_survives_ring_rotation() {
        let mut ob = OutputBuffer::new(8);
        ob.append("p", b"AAAA");
        ob.append("p", b"BBBB");
        ob.append("p", b"CCCC");
        // ring is 8 bytes — only "BBBBCCCC" remain, total_written = 12.
        assert_eq!(ob.total_written("p"), 12);
        let (chunk, new_total) = ob.read_since("p", 4);
        assert_eq!(new_total, 12);
        assert_eq!(chunk, b"BBBBCCCC");

        ob.append("p", b"DDDD");
        let (chunk2, new_total2) = ob.read_since("p", new_total);
        assert_eq!(new_total2, 16);
        assert_eq!(chunk2, b"DDDD");
    }

    #[test]
    fn read_since_clamps_offset_past_drained_window() {
        // Tight caps on both ring and full so we can exercise the clamp.
        let mut ob = OutputBuffer::new_with_caps(4, 4);
        ob.append("p", b"01234567");
        let (chunk, total) = ob.read_since("p", 0);
        assert_eq!(total, 8);
        assert_eq!(chunk, b"4567");
    }

    #[test]
    fn read_since_at_total_returns_empty() {
        let mut ob = OutputBuffer::new(64);
        ob.append("p", b"hello");
        let (chunk, total) = ob.read_since("p", 5);
        assert_eq!(total, 5);
        assert!(chunk.is_empty());
    }

    #[test]
    fn output_buffer_seed_precedes_live_output_with_consistent_offsets() {
        let mut buffer = OutputBuffer::new(64);
        buffer.prepend("p", b"restored history\n");
        let history_end = buffer.total_written("p");
        buffer.append("p", b"fresh banner\n");

        assert_eq!(
            buffer.get("p"),
            Some(&b"restored history\nfresh banner\n"[..])
        );
        assert_eq!(buffer.total_written("p"), history_end + 13);
        assert_eq!(
            buffer.read_since("p", 0),
            (
                b"restored history\nfresh banner\n".to_vec(),
                history_end + 13
            )
        );
        assert_eq!(
            buffer.read_since("p", history_end),
            (b"fresh banner\n".to_vec(), history_end + 13)
        );
    }

    #[test]
    fn output_buffer_seed_keeps_only_the_tail_and_adjusts_offsets() {
        let mut buffer = OutputBuffer::new_with_caps(6, 8);
        buffer.prepend("p", b"old history");
        buffer.append("p", b"fresh");

        assert_eq!(buffer.get("p"), Some(&b"yfresh"[..]));
        assert_eq!(buffer.get_full("p"), Some(&b"oryfresh"[..]));
        assert_eq!(buffer.total_written("p"), 16);
        assert_eq!(buffer.read_since("p", 11).0, b"fresh");
    }

    #[test]
    fn teardown_completion_wakes_on_child_and_reader_events() {
        let signal = Arc::new(PtyTeardownSignal::default());
        signal.mark_reader_started();
        let waiter = signal.clone();
        let (tx, rx) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            tx.send(waiter.wait_for_completion(Instant::now() + std::time::Duration::from_secs(1)))
                .unwrap();
        });

        signal.mark_child_exited();
        signal.mark_reader_finished();

        assert!(rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap()
            .is_ok());
        handle.join().unwrap();
    }

    #[test]
    fn teardown_completion_emit_before_wait_begin_succeeds() {
        let signal = Arc::new(PtyTeardownSignal::default());
        let emitted = signal.clone();
        let (emitted_tx, emitted_rx) = mpsc::channel();
        let emitter = std::thread::spawn(move || {
            emitted.mark_reader_started();
            emitted.mark_child_exited();
            emitted.mark_reader_finished();
            emitted_tx.send(()).unwrap();
        });

        emitted_rx.recv().unwrap();
        assert!(signal.wait_for_completion(Instant::now()).is_ok());
        emitter.join().unwrap();
    }

    #[test]
    fn teardown_failure_emit_before_wait_begin_fails() {
        let signal = Arc::new(PtyTeardownSignal::default());
        let emitted = signal.clone();
        let (emitted_tx, emitted_rx) = mpsc::channel();
        let emitter = std::thread::spawn(move || {
            emitted.mark_child_failed("reader failed".to_string());
            emitted_tx.send(()).unwrap();
        });

        emitted_rx.recv().unwrap();
        let error = signal
            .wait_for_completion(Instant::now())
            .expect_err("failure emitted before wait must remain a failure");
        assert!(error.contains("reader failed"), "{error}");
        emitter.join().unwrap();
    }

    #[test]
    fn teardown_deadline_is_a_failure() {
        let signal = PtyTeardownSignal::default();

        let error = signal
            .wait_for_completion(Instant::now())
            .expect_err("deadline must not report successful teardown");

        assert!(error.contains("deadline"), "{error}");
    }
}

/// A freshly opened PTY plus its child process, not yet registered in any manager.
/// Produced by `build_session` — the heavy, blocking step — so the manager lock
/// is not held while `openpty` / `CreateProcess` run.
pub struct BuiltSession {
    pub shell: String,
    pub session: Arc<PtySession>,
    _spawn_permit: super::spawn_gate::PtySpawnPermit,
}

/// Build a new PTY + child process without touching any manager state.
/// Safe to call from `spawn_blocking` — does blocking I/O (ConPTY + CreateProcess on Windows).
/// Env vars removed from every interactive tab PTY (see build_session for why).
pub const STRIPPED_PTY_ENV: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_CHILD_SESSION",
    "AI_AGENT",
    "NO_COLOR",
    "CI",
    "CONTINUOUS_INTEGRATION",
];

const INHERITED_ORCHESTRATION_ENV: &[&str] = &[
    "CODETERM_PLAN_ID",
    "CODETERM_TASK_ID",
    "CODETERM_LANE_BASE",
    "CODETERM_ROLE",
    "CODETERM_PARENT_TAB_ID",
    "CODETERM_WORKER_ID",
];

/// The single rule for putting the CodeTerm CLI directory on a PATH: every
/// existing copy is dropped, then it is prepended once. Both owners of that
/// prepend — the PTY environment here and the post-profile launch line — apply
/// this rule, so a tab carries the directory exactly once end to end.
pub fn path_with_cli_dir_first(
    inherited: &std::ffi::OsStr,
    cli_dir: &std::path::Path,
) -> Option<std::ffi::OsString> {
    let paths = std::iter::once(cli_dir.to_path_buf())
        .chain(std::env::split_paths(inherited).filter(|path| path != cli_dir));
    std::env::join_paths(paths).ok()
}

/// Directory holding the CLI named by `CODETERM_CLI_PATH`, if that names one.
pub fn cli_dir_of(cli_path: &std::ffi::OsStr) -> Option<std::path::PathBuf> {
    std::path::Path::new(cli_path)
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .map(std::path::Path::to_path_buf)
}

fn apply_pty_env(cmd: &mut CommandBuilder, env_vars: &HashMap<String, String>) {
    for key in INHERITED_ORCHESTRATION_ENV {
        cmd.env_remove(key);
    }

    for (key, value) in env_vars {
        cmd.env(key, value);
    }

    let cli_path = env_vars
        .get("CODETERM_CLI_PATH")
        .map(std::ffi::OsString::from)
        .or_else(|| std::env::var_os("CODETERM_CLI_PATH"));
    if let Some(cli_dir) = cli_path.as_deref().and_then(cli_dir_of) {
        let inherited_path = env_vars
            .get("PATH")
            .map(std::ffi::OsString::from)
            .or_else(|| std::env::var_os("PATH"))
            .unwrap_or_default();
        if let Some(path) = path_with_cli_dir_first(&inherited_path, &cli_dir) {
            cmd.env("PATH", path);
        }
    }

    for key in STRIPPED_PTY_ENV {
        cmd.env_remove(key);
    }
}

pub fn build_session(
    spawn_gate: PtySpawnGate,
    command: Option<String>,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
    env_vars: HashMap<String, String>,
) -> Result<BuiltSession, String> {
    let spawn_permit = spawn_gate.acquire()?;
    let pty_system = native_pty_system();
    let size = PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    };

    let pair = pty_system
        .openpty(size)
        .map_err(|e| format!("Failed to open PTY: {}", e))?;

    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;

    let (shell, mut cmd) = match &command {
        Some(c) => (c.clone(), CommandBuilder::new(c)),
        None => {
            let s = detect_default_shell();
            #[cfg(target_os = "windows")]
            {
                (s.clone(), CommandBuilder::new(&s))
            }
            #[cfg(not(target_os = "windows"))]
            {
                (s, CommandBuilder::new_default_prog())
            }
        }
    };

    if let Some(dir) = cwd {
        cmd.cwd(dir);
    }

    cmd.env("CODETERM", "1");
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("CLICOLOR", "1");
    cmd.env("TERM_PROGRAM", "CodeTerm");
    cmd.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));

    #[cfg(not(target_os = "windows"))]
    {
        if std::env::var("LANG").map_or(true, |v| v.is_empty()) {
            cmd.env("LANG", "en_US.UTF-8");
        }
        cmd.env("LC_CTYPE", "UTF-8");
    }

    // portable-pty resolves the default program from $SHELL before consulting the
    // account database; pin the shell we resolved so a stale inherited value loses.
    #[cfg(not(target_os = "windows"))]
    if command.is_none() {
        cmd.env("SHELL", &shell);
    }

    super::shell_integration::apply_env(&mut cmd, &shell);

    // Env vars stripped from every interactive tab PTY. CLAUDECODE / CLAUDE_CODE_*:
    apply_pty_env(&mut cmd, &env_vars);

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("Failed to spawn '{}': {}", shell, e))?;

    let write_queue = WriteQueue::new(writer);

    let session = Arc::new(PtySession {
        generation: next_session_generation(),
        pair: StdMutex::new(Some(pair)),
        size: StdMutex::new((cols, rows)),
        reader: StdMutex::new(Some(reader)),
        write_queue,
        guarded_input: GuardedInputGate::default(),
        child: StdMutex::new(child),
        backend_reader_tx: StdMutex::new(None),
        backend_reader_rx: StdMutex::new(None),
        backend_reader_active: AtomicBool::new(false),
        teardown: PtyTeardownSignal::default(),
        child_exit: Arc::default(),
    });
    if let Ok(child) = session.child.lock() {
        let label = format!("pid={:?}", child.process_id());
        super::child_exit::watch(
            &**child,
            Arc::downgrade(&session),
            session.child_exit.clone(),
            label,
        );
    } else {
        session.child_exit.mark_unobservable();
    }

    Ok(BuiltSession {
        shell,
        session,
        _spawn_permit: spawn_permit,
    })
}

/// Manages PTY sessions. Sessions are stored behind Arc for shared access.
pub struct PtyManager {
    sessions: HashMap<String, Arc<PtySession>>,
    spawn_gate: PtySpawnGate,
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            spawn_gate: PtySpawnGate::default(),
        }
    }

    pub fn spawn_gate(&self) -> PtySpawnGate {
        self.spawn_gate.clone()
    }

    pub fn spawn(
        &mut self,
        command: Option<String>,
        cwd: Option<String>,
        cols: u16,
        rows: u16,
        env_vars: HashMap<String, String>,
    ) -> Result<(String, Arc<PtySession>), String> {
        self.spawn_with_id(None, command, cwd, cols, rows, env_vars)
    }

    pub fn spawn_with_id(
        &mut self,
        custom_id: Option<String>,
        command: Option<String>,
        cwd: Option<String>,
        cols: u16,
        rows: u16,
        env_vars: HashMap<String, String>,
    ) -> Result<(String, Arc<PtySession>), String> {
        let built = build_session(self.spawn_gate.clone(), command, cwd, cols, rows, env_vars)?;
        Ok(self.register_session(custom_id, built))
    }

    /// Insert a pre-built session into the manager. Holds the manager lock only
    /// long enough to insert into the HashMap — the heavy `openpty` +
    /// `CreateProcess` work has already happened in `build_session`.
    pub fn register_session(
        &mut self,
        custom_id: Option<String>,
        built: BuiltSession,
    ) -> (String, Arc<PtySession>) {
        let BuiltSession {
            shell,
            session,
            _spawn_permit,
        } = built;
        let pty_id = custom_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let child_pid = session
            .child
            .lock()
            .ok()
            .and_then(|child| child.process_id());
        log::info!(
            "Spawned PTY {} with shell '{}' child_pid={:?}",
            pty_id,
            shell,
            child_pid
        );
        record_spawn_time(&pty_id, &shell);
        self.sessions.insert(pty_id.clone(), session.clone());
        (pty_id, session)
    }

    pub fn get_session(&self, pty_id: &str) -> Option<Arc<PtySession>> {
        self.sessions.get(pty_id).cloned()
    }

    pub fn session_generation_matches(&self, pty_id: &str, generation: u64) -> bool {
        self.sessions
            .get(pty_id)
            .is_some_and(|session| session.generation == generation)
    }

    /// Write to PTY via the serialized write queue.
    pub fn write(&self, pty_id: &str, data: &[u8]) -> Result<(), String> {
        let session = self
            .sessions
            .get(pty_id)
            .ok_or_else(|| format!("PTY not found: {}", pty_id))?;
        crate::pty::capture::CaptureRegistry::global().tap_input(pty_id, "pty.write", data);
        session.write_queue.send(data.to_vec())
    }

    pub fn write_and_flush(
        &self,
        pty_id: &str,
        data: &[u8],
        timeout: std::time::Duration,
    ) -> Result<(), String> {
        let session = self
            .sessions
            .get(pty_id)
            .ok_or_else(|| format!("PTY not found: {pty_id}"))?;
        crate::pty::capture::CaptureRegistry::global().tap_input(pty_id, "pty.write", data);
        session.write_queue.send_and_wait(data.to_vec(), timeout)
    }

    pub fn write_frames_and_flush(
        &self,
        pty_id: &str,
        frames: Vec<Vec<u8>>,
        timeout: std::time::Duration,
    ) -> Result<(), String> {
        let session = self
            .sessions
            .get(pty_id)
            .ok_or_else(|| format!("PTY not found: {pty_id}"))?;
        for frame in &frames {
            crate::pty::capture::CaptureRegistry::global().tap_input(pty_id, "pty.write", frame);
        }
        session.write_queue.send_frames_and_wait(frames, timeout)
    }

    pub fn rearm_guarded_input(&self, pty_id: &str) -> Result<(), String> {
        let session = self
            .sessions
            .get(pty_id)
            .ok_or_else(|| format!("PTY not found: {pty_id}"))?;
        session.guarded_input.rearm();
        Ok(())
    }

    pub fn invalidate_guarded_input(&self, pty_id: &str) {
        if let Some(session) = self.sessions.get(pty_id) {
            session.guarded_input.invalidate();
        }
    }

    pub fn issue_guarded_input_permit(&self, pty_id: &str) -> Result<GuardedInputPermit, String> {
        let session = self
            .sessions
            .get(pty_id)
            .ok_or_else(|| format!("PTY not found: {pty_id}"))?;
        session
            .guarded_input
            .issue()
            .ok_or_else(|| "agent_unavailable: agent input is not accepting".to_string())
    }

    pub fn write_and_flush_guarded(
        &self,
        pty_id: &str,
        data: &[u8],
        permit: GuardedInputPermit,
        timeout: std::time::Duration,
    ) -> Result<(), String> {
        let session = self
            .sessions
            .get(pty_id)
            .ok_or_else(|| format!("PTY not found: {pty_id}"))?;
        session
            .write_and_flush_guarded(data.to_vec(), permit, timeout)
            .map(|()| {
                crate::pty::capture::CaptureRegistry::global().tap_input(pty_id, "pty.write", data);
            })
    }

    /// Resize PTY
    pub fn resize(&mut self, pty_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let session = self
            .sessions
            .get(pty_id)
            .ok_or_else(|| format!("PTY not found: {}", pty_id))?;
        let _ = session.resize(cols, rows)?;
        Ok(())
    }

    /// Current (cols, rows) of a PTY, as last applied by spawn or resize.
    pub fn size(&self, pty_id: &str) -> Option<(u16, u16)> {
        let session = self.sessions.get(pty_id)?;
        session.size.lock().ok().map(|s| *s)
    }

    /// Kill/remove PTY. Returns error if PTY not found (already killed).
    pub fn kill(&mut self, pty_id: &str) -> Result<(), String> {
        let reaper = self.begin_kill(pty_id)?;
        reaper.reap_in_background();
        Ok(())
    }

    pub fn begin_kill(&mut self, pty_id: &str) -> Result<PtyReaper, String> {
        let session = self
            .sessions
            .remove(pty_id)
            .ok_or_else(|| format!("PTY {} not found", pty_id))?;
        session.guarded_input.invalidate();
        let kill_error = match session.child.lock() {
            Ok(mut child) => child
                .kill()
                .err()
                .filter(kill_failure_blocks_teardown)
                .map(|error| error.to_string()),
            Err(error) => Some(format!("child mutex poisoned: {error}")),
        };
        session.write_queue.close();
        if let Ok(mut reader) = session.reader.lock() {
            let _ = reader.take();
        }
        if let Ok(mut tx) = session.backend_reader_tx.lock() {
            let _ = tx.take();
        }
        if let Ok(mut rx) = session.backend_reader_rx.lock() {
            let _ = rx.take();
        }
        // ConPTY handle is closed by the reaper's background thread, not here —
        // ClosePseudoConsole can block (see teardown.rs) and must not stall the caller.
        log::info!("Killed PTY {} process tree; ConPTY teardown queued", pty_id);
        Ok(PtyReaper {
            pty_id: pty_id.to_string(),
            session,
            kill_error,
        })
    }

    pub fn begin_shutdown(&mut self) -> Result<Vec<PtyReaper>, String> {
        self.spawn_gate.quiesce();
        let pty_ids = self.sessions.keys().cloned().collect::<Vec<_>>();
        pty_ids
            .into_iter()
            .map(|pty_id| self.begin_kill(&pty_id))
            .collect()
    }

    pub fn remove(&mut self, pty_id: &str) {
        self.sessions.remove(pty_id);
        log::info!("Removed PTY {}", pty_id);
    }

    /// Max buffered chunks in the backend→frontend channel.
    /// 1024 × 8KB max chunk = 8MB ceiling per PTY for hidden tabs.
    const BACKEND_CHANNEL_CAPACITY: usize = 1024;

    /// Set up the backend reader channel for a PTY.
    /// Returns the sender for the backend reader to use.
    /// After this call, read_pty will read from the channel instead of the raw PTY.
    pub fn setup_backend_channel(
        &self,
        pty_id: &str,
    ) -> Option<tokio::sync::mpsc::Sender<Vec<u8>>> {
        let session = self.sessions.get(pty_id)?;
        if let Some(existing) = session.backend_reader_tx.lock().unwrap().clone() {
            return Some(existing);
        }
        let (tx, rx) = tokio::sync::mpsc::channel::<Vec<u8>>(Self::BACKEND_CHANNEL_CAPACITY);
        *session.backend_reader_tx.lock().unwrap() = Some(tx.clone());
        *session.backend_reader_rx.lock().unwrap() = Some(rx);
        Some(tx)
    }

    pub fn has_backend_reader(&self, pty_id: &str) -> bool {
        self.sessions
            .get(pty_id)
            .and_then(|session| session.backend_reader_tx.lock().unwrap().clone())
            .is_some()
    }
}

fn detect_default_shell() -> String {
    super::login_shell::resolve_login_shell()
}

/// ESRCH means the tree is already gone, which is what teardown asked for.
/// EPERM and every other errno stay real failures that must block a shutdown.
#[cfg(unix)]
fn kill_failure_blocks_teardown(error: &std::io::Error) -> bool {
    error.raw_os_error() != Some(libc::ESRCH)
}

#[cfg(not(unix))]
fn kill_failure_blocks_teardown(_error: &std::io::Error) -> bool {
    true
}

#[cfg(test)]
mod tests;
