use portable_pty::{native_pty_system, CommandBuilder, PtyPair, PtySize};
use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::Instant;

use parking_lot::Mutex as PlMutex;

use super::write_queue::WriteQueue;

pub struct SpawnMark {
    pub when: Instant,
    pub shell: String,
}

fn spawn_times() -> &'static PlMutex<HashMap<String, SpawnMark>> {
    static SPAWN_TIMES: OnceLock<PlMutex<HashMap<String, SpawnMark>>> = OnceLock::new();
    SPAWN_TIMES.get_or_init(|| PlMutex::new(HashMap::new()))
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
    pub pair: StdMutex<PtyPair>,
    pub size: StdMutex<(u16, u16)>,
    pub reader: StdMutex<Option<Box<dyn Read + Send>>>,
    pub write_queue: WriteQueue,
    pub child: StdMutex<Box<dyn portable_pty::Child + Send + Sync>>,
    /// Backend reader channel (bounded). When Some, a backend reader is active and sending bytes here.
    /// read_pty reads from this instead of the raw PTY reader to avoid competing reads.
    /// Bounded to prevent memory leaks when frontend isn't consuming (hidden tabs).
    pub backend_reader_tx: StdMutex<Option<tokio::sync::mpsc::Sender<Vec<u8>>>>,
    pub backend_reader_rx: StdMutex<Option<tokio::sync::mpsc::Receiver<Vec<u8>>>>,
}

impl PtySession {
    pub fn resize(&self, cols: u16, rows: u16) -> Result<bool, String> {
        let mut size = self.size.lock().map_err(|e| e.to_string())?;
        if *size == (cols, rows) {
            return Ok(false);
        }
        let pair = self.pair.lock().map_err(|e| e.to_string())?;
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
}

/// Ring buffer storing last N bytes of PTY output per pane.
/// Also maintains a full history (capped at MAX_FULL_SIZE) for chat view parsing.
///
/// Screen parsing (VT100) lives in `ScreenState` behind a separate lock
/// to avoid contention between output processing and chat view reads.
pub struct OutputBuffer {
    buffers: HashMap<String, Vec<u8>>,
    /// Full output history (capped at MAX_FULL_SIZE per PTY). Used by chat view parser.
    full_buffers: HashMap<String, Vec<u8>>,
    /// Monotonic count of bytes ever written per pty, surviving ring-buffer
    /// rotation. SSE clients use this as a stable absolute offset.
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

    /// Total bytes ever written to this pty (monotonic).
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

#[cfg(test)]
mod output_buffer_tests {
    use super::*;

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
}

/// A freshly opened PTY plus its child process, not yet registered in any manager.
/// Produced by `build_session` — the heavy, blocking step — so the manager lock
/// is not held while `openpty` / `CreateProcess` run.
pub struct BuiltSession {
    pub shell: String,
    pub session: Arc<PtySession>,
}

/// Build a new PTY + child process without touching any manager state.
/// Safe to call from `spawn_blocking` — does blocking I/O (ConPTY + CreateProcess on Windows).
/// Env vars removed from every interactive pane PTY (see build_session for why).
pub const STRIPPED_PTY_ENV: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_CHILD_SESSION",
    "NO_COLOR",
    "CI",
    "CONTINUOUS_INTEGRATION",
];

pub fn build_session(
    command: Option<String>,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
    env_vars: HashMap<String, String>,
) -> Result<BuiltSession, String> {
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

    let (shell, mut cmd) = match command {
        Some(c) => (c.clone(), CommandBuilder::new(&c)),
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

    super::shell_integration::apply_env(&mut cmd, &shell);

    for (key, value) in &env_vars {
        cmd.env(key, value);
    }

    // Env vars stripped from every interactive pane PTY. CLAUDECODE / CLAUDE_CODE_*:
    // Claude 2.1.x treats them as "I'm a sub-session" and writes no transcript.
    // CI / CONTINUOUS_INTEGRATION: an interactive pane is never a CI job, and an
    // inherited CI=true makes TUIs (cursor-agent etc.) go non-interactive and hide
    // their input bar even though stdout is a real TTY. NO_COLOR: we want color.
    for key in STRIPPED_PTY_ENV {
        cmd.env_remove(key);
    }

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("Failed to spawn '{}': {}", shell, e))?;

    let write_queue = WriteQueue::new(writer);

    let session = Arc::new(PtySession {
        pair: StdMutex::new(pair),
        size: StdMutex::new((cols, rows)),
        reader: StdMutex::new(Some(reader)),
        write_queue,
        child: StdMutex::new(child),
        backend_reader_tx: StdMutex::new(None),
        backend_reader_rx: StdMutex::new(None),
    });

    Ok(BuiltSession { shell, session })
}

/// Manages PTY sessions. Sessions are stored behind Arc for shared access.
pub struct PtyManager {
    sessions: HashMap<String, Arc<PtySession>>,
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
        }
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
        let built = build_session(command, cwd, cols, rows, env_vars)?;
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
        let BuiltSession { shell, session } = built;
        let pty_id = custom_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        log::info!("Spawned PTY {} with shell '{}'", pty_id, shell);
        record_spawn_time(&pty_id, &shell);
        self.sessions.insert(pty_id.clone(), session.clone());
        (pty_id, session)
    }

    pub fn get_session(&self, pty_id: &str) -> Option<Arc<PtySession>> {
        self.sessions.get(pty_id).cloned()
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

    /// Resize PTY
    pub fn resize(&mut self, pty_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let session = self
            .sessions
            .get(pty_id)
            .ok_or_else(|| format!("PTY not found: {}", pty_id))?;
        let _ = session.resize(cols, rows)?;
        Ok(())
    }

    /// Kill/remove PTY. Returns error if PTY not found (already killed).
    pub fn kill(&mut self, pty_id: &str) -> Result<(), String> {
        let session = self
            .sessions
            .remove(pty_id)
            .ok_or_else(|| format!("PTY {} not found", pty_id))?;
        match session.child.lock() {
            Ok(mut child) => {
                let _ = child.kill();
            }
            Err(e) => log::warn!("PTY {} child mutex poisoned during kill: {}", pty_id, e),
        }
        // Reap in background — wait() can block indefinitely if descendants ignore SIGKILL or zombie isn't reaped promptly. Move session ownership to
        // background thread so PTY lock is released immediately.
        let id = pty_id.to_string();
        std::thread::spawn(move || {
            match session.child.lock() {
                Ok(mut c) => {
                    let _ = c.wait();
                }
                Err(_) => {}
            }
            drop(session);
            log::info!("PTY {} child reaped", id);
        });
        log::info!(
            "Killed PTY {} (SIGKILL sent, reaping in background)",
            pty_id
        );
        Ok(())
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
    #[cfg(target_os = "windows")]
    {
        // Avoid the WindowsApps stub for pwsh.exe — Store loader serialises ConPTY spawns for 10–30s. Prefer real installs.
        let candidates = [
            r"C:\Program Files\PowerShell\7\pwsh.exe",
            r"C:\Program Files (x86)\PowerShell\7\pwsh.exe",
        ];
        for c in candidates.iter() {
            if std::path::Path::new(c).is_file() {
                return (*c).to_string();
            }
        }
        if let Some(path) = which_exe("pwsh.exe") {
            return path;
        }
        if let Some(path) = which_exe("powershell.exe") {
            return path;
        }
        std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
    }

    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
    }
}

#[cfg(target_os = "windows")]
fn which_exe(name: &str) -> Option<String> {
    let path = std::env::var("PATH").ok()?;
    for dir in path.split(';') {
        if dir.is_empty() {
            continue;
        }
        // Skip WindowsApps App Execution Alias dir — see detect_default_shell.
        if dir.to_ascii_lowercase().contains(r"\microsoft\windowsapps") {
            continue;
        }
        let full = std::path::Path::new(dir).join(name);
        if full.is_file() {
            return Some(full.to_string_lossy().into_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kill_terminates_child_and_removes_session() {
        let mut manager = PtyManager::new();
        let (pty_id, session) = manager
            .spawn(None, None, 80, 24, HashMap::new())
            .expect("spawn default shell");

        let pid = session.child.lock().expect("child mutex").process_id();
        assert!(pid.is_some(), "spawned child should expose a pid");

        manager.kill(&pty_id).expect("kill should succeed");

        assert!(
            manager.get_session(&pty_id).is_none(),
            "session should be gone from manager after kill"
        );

        let mut child = session.child.lock().expect("child mutex");
        let status = child
            .try_wait()
            .expect("try_wait after kill should not error");
        assert!(
            status.is_some(),
            "child must have exited after PtyManager::kill (was: {:?})",
            status
        );
    }

    #[test]
    fn kill_returns_err_for_unknown_pty() {
        let mut manager = PtyManager::new();
        let result = manager.kill("nonexistent-pty-id");
        assert!(result.is_err(), "kill on unknown id must return Err");
    }

    /// Interactive panes must never look like a CI job: an inherited CI=true makes
    /// cursor-agent (and other is-ci-aware TUIs) go non-interactive and hide their
    /// input bar even with a real TTY. Guard against the strip list regressing.
    #[test]
    fn stripped_pty_env_drops_ci_markers() {
        assert!(STRIPPED_PTY_ENV.contains(&"CI"), "CI must be stripped");
        assert!(
            STRIPPED_PTY_ENV.contains(&"CONTINUOUS_INTEGRATION"),
            "CONTINUOUS_INTEGRATION must be stripped"
        );
        assert!(STRIPPED_PTY_ENV.contains(&"CLAUDECODE"));
        assert!(STRIPPED_PTY_ENV.contains(&"NO_COLOR"));
    }

    /// `launch_agent_in_background` relies on `write` returning `Err("PTY not
    /// found: <id>")` so the success log is gated and the misleading "agent
    /// launched" line never fires when the PTY lookup races and misses.
    #[test]
    fn write_returns_err_for_unknown_pty() {
        let manager = PtyManager::new();
        let err = manager
            .write("missing-pty-id", b"hello")
            .expect_err("write on unknown id must return Err");
        assert!(
            err.contains("PTY not found"),
            "error must signal missing PTY (got {:?})",
            err
        );
        assert!(
            err.contains("missing-pty-id"),
            "error must include the offending id (got {:?})",
            err
        );
    }
}
