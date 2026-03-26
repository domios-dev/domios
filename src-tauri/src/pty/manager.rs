use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtyPair, PtySize, PtySystem};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex as StdMutex};

/// A PTY session — stores the pair, reader, writer, child.
pub struct PtySession {
    pub pair: StdMutex<PtyPair>,
    pub reader: StdMutex<Option<Box<dyn Read + Send>>>,
    pub writer: StdMutex<Box<dyn Write + Send>>,
    pub child: StdMutex<Box<dyn portable_pty::Child + Send + Sync>>,
}

/// Ring buffer storing last N bytes of PTY output per pane
pub struct OutputBuffer {
    buffers: HashMap<String, Vec<u8>>,
    max_size: usize,
}

impl OutputBuffer {
    pub fn new(max_size: usize) -> Self {
        Self { buffers: HashMap::new(), max_size }
    }

    pub fn append(&mut self, pty_id: &str, data: &[u8]) {
        let buf = self.buffers.entry(pty_id.to_string()).or_default();
        buf.extend_from_slice(data);
        if buf.len() > self.max_size {
            let drain = buf.len() - self.max_size;
            buf.drain(..drain);
        }
    }

    pub fn get(&self, pty_id: &str) -> Option<&[u8]> {
        self.buffers.get(pty_id).map(|v| v.as_slice())
    }

    pub fn get_text(&self, pty_id: &str) -> Option<String> {
        self.get(pty_id).map(|b| String::from_utf8_lossy(b).to_string())
    }

    pub fn remove(&mut self, pty_id: &str) {
        self.buffers.remove(pty_id);
    }
}

/// Manages PTY sessions. Sessions are stored behind Arc for shared access.
pub struct PtyManager {
    sessions: HashMap<String, Arc<PtySession>>,
}

impl PtyManager {
    pub fn new() -> Self {
        Self { sessions: HashMap::new() }
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
        let pty_system = native_pty_system();
        let size = PtySize { rows, cols, pixel_width: 0, pixel_height: 0 };

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
                    // On Windows, new_default_prog() always uses COMSPEC (cmd.exe).
                    // We want PowerShell, so spawn it explicitly.
                    (s.clone(), CommandBuilder::new(&s))
                }
                #[cfg(not(target_os = "windows"))]
                {
                    // On Unix, new_default_prog() spawns a login shell (argv[0] prefixed
                    // with '-'), which sources .zprofile/.bash_profile and sets up PATH.
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
        cmd.env("TERM_PROGRAM", "CodeTerm");
        cmd.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));

        // Ensure UTF-8 locale for proper Unicode/Cyrillic support (Unix only)
        #[cfg(not(target_os = "windows"))]
        {
            if std::env::var("LANG").map_or(true, |v| v.is_empty()) {
                cmd.env("LANG", "en_US.UTF-8");
            }
            cmd.env("LC_CTYPE", "UTF-8");
        }

        // Invisible shell integration: OSC 7 CWD tracking via env vars
        super::shell_integration::apply_env(&mut cmd, &shell);

        for (key, value) in &env_vars {
            cmd.env(key, value);
        }

        // Remove vars that prevent tools from running inside CodeTerm
        cmd.env_remove("CLAUDECODE");

        let child = pair.slave.spawn_command(cmd)
            .map_err(|e| format!("Failed to spawn '{}': {}", shell, e))?;

        let pty_id = custom_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        log::info!("Spawned PTY {} with shell '{}'", pty_id, shell);

        let session = Arc::new(PtySession {
            pair: StdMutex::new(pair),
            reader: StdMutex::new(Some(reader)),
            writer: StdMutex::new(writer),
            child: StdMutex::new(child),
        });

        self.sessions.insert(pty_id.clone(), session.clone());
        Ok((pty_id, session))
    }

    pub fn get_session(&self, pty_id: &str) -> Option<Arc<PtySession>> {
        self.sessions.get(pty_id).cloned()
    }

    /// Write to PTY (for sync callers like socket/API)
    pub fn write(&mut self, pty_id: &str, data: &[u8]) -> Result<(), String> {
        let session = self.sessions.get(pty_id)
            .ok_or_else(|| format!("PTY not found: {}", pty_id))?;
        let mut writer = session.writer.lock().map_err(|e| e.to_string())?;
        writer.write_all(data).map_err(|e| format!("Write failed: {}", e))?;
        writer.flush().map_err(|e| format!("Flush failed: {}", e))?;
        Ok(())
    }

    /// Resize PTY
    pub fn resize(&mut self, pty_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let session = self.sessions.get(pty_id)
            .ok_or_else(|| format!("PTY not found: {}", pty_id))?;
        let pair = session.pair.lock().map_err(|e| e.to_string())?;
        pair.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        }).map_err(|e| format!("Resize failed: {}", e))?;
        Ok(())
    }

    /// Kill/remove PTY
    pub fn kill(&mut self, pty_id: &str) -> Result<(), String> {
        self.sessions.remove(pty_id);
        log::info!("Killed PTY {}", pty_id);
        Ok(())
    }

    pub fn remove(&mut self, pty_id: &str) {
        self.sessions.remove(pty_id);
        log::info!("Removed PTY {}", pty_id);
    }
}

fn detect_default_shell() -> String {
    #[cfg(target_os = "windows")]
    {
        if let Ok(comspec) = std::env::var("COMSPEC") {
            if which_exists("powershell.exe") {
                return "powershell.exe".to_string();
            }
            return comspec;
        }
        "cmd.exe".to_string()
    }

    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
    }
}

#[cfg(target_os = "windows")]
fn which_exists(name: &str) -> bool {
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(';') {
            let full = std::path::Path::new(dir).join(name);
            if full.exists() {
                return true;
            }
        }
    }
    false
}
