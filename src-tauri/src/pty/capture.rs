//! Dev-only PTY capture tap → asciicast v2 `.cast`.
//!
//! When capture is armed on a pane, raw PTY output bytes (ANSI intact) plus a
//! per-chunk monotonic timestamp are teed into a `.cast` file off the PTY pump.
//! ZERO cost when not capturing: a single relaxed atomic load gates the fast path.
//! All file IO happens on a dedicated writer thread fed by an unbounded channel,
//! never on the PTY read loop or the Tauri async runtime.

use std::collections::HashMap;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{LazyLock, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

/// Writes an asciicast v2 stream: a JSON header line followed by one NDJSON
/// event row per chunk. Output rows split only on UTF-8 char boundaries so the
/// concatenated `"o"` payloads round-trip byte-identical for any valid-UTF-8
/// stream — incomplete trailing bytes (a multibyte char split across PTY reads)
/// are carried to the next row.
pub struct CastWriter {
    out: BufWriter<std::fs::File>,
    pending: Vec<u8>,
}

impl CastWriter {
    pub fn create(path: &Path, cols: u16, rows: u16, timestamp: u64) -> io::Result<Self> {
        let file = std::fs::File::create(path)?;
        let mut out = BufWriter::new(file);
        writeln!(
            out,
            "{{\"version\": 2, \"width\": {}, \"height\": {}, \"timestamp\": {}}}",
            cols, rows, timestamp
        )?;
        Ok(Self {
            out,
            pending: Vec::new(),
        })
    }

    pub fn write_output(&mut self, t: f64, data: &[u8]) -> io::Result<()> {
        self.pending.extend_from_slice(data);
        let valid = utf8_valid_prefix(&self.pending);
        if valid == 0 {
            return Ok(());
        }
        let text = String::from_utf8_lossy(&self.pending[..valid]).into_owned();
        self.pending.drain(..valid);
        let line = serde_json::json!([t, "o", text]);
        writeln!(self.out, "{}", line)
    }

    pub fn write_resize(&mut self, t: f64, cols: u16, rows: u16) -> io::Result<()> {
        self.flush_pending(t)?;
        let line = serde_json::json!([t, "r", format!("{}x{}", cols, rows)]);
        writeln!(self.out, "{}", line)
    }

    pub fn finish(mut self) -> io::Result<()> {
        // Genuinely invalid trailing bytes (never completed) — flush lossily so
        // no chunk is silently dropped; valid streams leave `pending` empty here.
        let t = 0.0;
        self.flush_pending(t)?;
        self.out.flush()
    }

    fn flush_pending(&mut self, t: f64) -> io::Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let text = String::from_utf8_lossy(&self.pending).into_owned();
        self.pending.clear();
        let line = serde_json::json!([t, "o", text]);
        writeln!(self.out, "{}", line)
    }
}

/// Largest prefix length of `buf` that is valid UTF-8.
fn utf8_valid_prefix(buf: &[u8]) -> usize {
    match std::str::from_utf8(buf) {
        Ok(_) => buf.len(),
        Err(e) => e.valid_up_to(),
    }
}

enum CaptureMsg {
    Output(f64, Vec<u8>),
    Resize(f64, u16, u16),
}

struct ActiveCapture {
    tx: Sender<CaptureMsg>,
    start: Instant,
    out: PathBuf,
    writer: Option<JoinHandle<()>>,
}

/// Per-pty capture registry. The `active` counter is the zero-cost gate: when it
/// is 0 the tap returns before touching the map lock.
pub struct CaptureRegistry {
    active: AtomicUsize,
    map: Mutex<HashMap<String, ActiveCapture>>,
}

static REGISTRY: LazyLock<CaptureRegistry> = LazyLock::new(CaptureRegistry::new);

impl CaptureRegistry {
    fn new() -> Self {
        Self {
            active: AtomicUsize::new(0),
            map: Mutex::new(HashMap::new()),
        }
    }

    pub fn global() -> &'static CaptureRegistry {
        &REGISTRY
    }

    #[inline]
    pub fn is_capturing(&self) -> bool {
        self.active.load(Ordering::Relaxed) > 0
    }

    /// Arm capture on `pty_id`, writing to `out`. Spawns the dedicated writer
    /// thread and writes the header immediately.
    pub fn start(
        &self,
        pty_id: &str,
        out: PathBuf,
        cols: u16,
        rows: u16,
        timestamp: u64,
    ) -> io::Result<()> {
        let mut map = self.map.lock().unwrap();
        if map.contains_key(pty_id) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "capture already active for this pane",
            ));
        }
        let writer = CastWriter::create(&out, cols, rows, timestamp)?;
        let (tx, rx) = mpsc::channel::<CaptureMsg>();
        let handle = std::thread::spawn(move || {
            let mut w = writer;
            while let Ok(msg) = rx.recv() {
                let r = match msg {
                    CaptureMsg::Output(t, data) => w.write_output(t, &data),
                    CaptureMsg::Resize(t, c, r) => w.write_resize(t, c, r),
                };
                if r.is_err() {
                    break;
                }
            }
            let _ = w.finish();
        });
        map.insert(
            pty_id.to_string(),
            ActiveCapture {
                tx,
                start: Instant::now(),
                out,
                writer: Some(handle),
            },
        );
        self.active.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Disarm capture on `pty_id`, flushing and closing the `.cast`. Returns the
    /// output path so the caller can render the sidecar `screen.txt`.
    pub fn stop(&self, pty_id: &str) -> Option<PathBuf> {
        let mut cap = {
            let mut map = self.map.lock().unwrap();
            map.remove(pty_id)?
        };
        self.active.fetch_sub(1, Ordering::Relaxed);
        let out = cap.out.clone();
        // Drop tx so the writer thread sees the channel close, then join to
        // guarantee the file is fully flushed before we return.
        let handle = cap.writer.take();
        drop(cap);
        if let Some(h) = handle {
            let _ = h.join();
        }
        Some(out)
    }

    #[inline]
    pub fn tap_output(&self, pty_id: &str, data: &[u8]) {
        if !self.is_capturing() {
            return;
        }
        let map = self.map.lock().unwrap();
        if let Some(cap) = map.get(pty_id) {
            let t = cap.start.elapsed().as_secs_f64();
            let _ = cap.tx.send(CaptureMsg::Output(t, data.to_vec()));
        }
    }

    #[inline]
    pub fn tap_resize(&self, pty_id: &str, cols: u16, rows: u16) {
        if !self.is_capturing() {
            return;
        }
        let map = self.map.lock().unwrap();
        if let Some(cap) = map.get(pty_id) {
            let t = cap.start.elapsed().as_secs_f64();
            let _ = cap.tx.send(CaptureMsg::Resize(t, cols, rows));
        }
    }
}

#[cfg(test)]
mod tests;
