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

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

/// Writes an asciicast v2 stream: a JSON header line followed by one NDJSON
/// event row per chunk. Output rows split only on UTF-8 char boundaries so the
/// concatenated `"o"` payloads round-trip byte-identical for any valid-UTF-8
/// stream. Truly invalid bytes are emitted as `"o+b64"` rows so the capture
/// remains byte-exact without forcing invalid UTF-8 into asciicast text rows.
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
        self.flush_decodable(t, false)
    }

    pub fn write_resize(&mut self, t: f64, cols: u16, rows: u16) -> io::Result<()> {
        self.flush_pending(t)?;
        let line = serde_json::json!([t, "r", format!("{}x{}", cols, rows)]);
        writeln!(self.out, "{}", line)
    }

    pub fn write_input(&mut self, t: f64, source: &str, data: &[u8]) -> io::Result<()> {
        self.flush_pending(t)?;
        let text = std::str::from_utf8(data).ok();
        let line = serde_json::json!([
            t,
            "i",
            {
                "source": source,
                "bytes_b64": BASE64.encode(data),
                "text": text,
            }
        ]);
        writeln!(self.out, "{}", line)
    }

    pub fn finish(mut self) -> io::Result<()> {
        self.flush_decodable(0.0, true)?;
        self.out.flush()
    }

    fn flush_pending(&mut self, t: f64) -> io::Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        self.flush_decodable(t, true)
    }

    fn flush_decodable(&mut self, t: f64, finish: bool) -> io::Result<()> {
        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(text) => {
                    if !text.is_empty() {
                        let line = serde_json::json!([t, "o", text]);
                        writeln!(self.out, "{}", line)?;
                    }
                    self.pending.clear();
                    return Ok(());
                }
                Err(e) => {
                    let valid = e.valid_up_to();
                    if valid > 0 {
                        let text = std::str::from_utf8(&self.pending[..valid]).unwrap();
                        let line = serde_json::json!([t, "o", text]);
                        writeln!(self.out, "{}", line)?;
                        self.pending.drain(..valid);
                        continue;
                    }
                    match e.error_len() {
                        Some(len) => {
                            let raw: Vec<u8> = self.pending.drain(..len).collect();
                            self.write_raw_b64(t, &raw)?;
                        }
                        None if finish => {
                            let raw = std::mem::take(&mut self.pending);
                            self.write_raw_b64(t, &raw)?;
                            return Ok(());
                        }
                        None => return Ok(()),
                    }
                }
            }
        }
    }

    fn write_raw_b64(&mut self, t: f64, data: &[u8]) -> io::Result<()> {
        if data.is_empty() {
            return Ok(());
        }
        let line = serde_json::json!([t, "o+b64", BASE64.encode(data)]);
        writeln!(self.out, "{}", line)
    }
}

/// Decode capture output rows into the original output bytes.
pub fn decode_output_event(code: &str, data: &str) -> io::Result<Option<Vec<u8>>> {
    match code {
        "o" => Ok(Some(data.as_bytes().to_vec())),
        "o+b64" => BASE64
            .decode(data)
            .map(Some)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
        _ => Ok(None),
    }
}

enum CaptureMsg {
    Output(f64, Vec<u8>),
    Resize(f64, u16, u16),
    Input(f64, String, Vec<u8>),
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
        prefill: Vec<u8>,
    ) -> io::Result<()> {
        let mut map = self.map.lock().unwrap();
        if map.contains_key(pty_id) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "capture already active for this Tab",
            ));
        }
        let writer = CastWriter::create(&out, cols, rows, timestamp)?;
        let (tx, rx) = mpsc::channel::<CaptureMsg>();
        let handle = std::thread::spawn(move || {
            let mut w = writer;
            if !prefill.is_empty() && w.write_output(0.0, &prefill).is_err() {
                return;
            }
            while let Ok(msg) = rx.recv() {
                let r = match msg {
                    CaptureMsg::Output(t, data) => w.write_output(t, &data),
                    CaptureMsg::Resize(t, c, r) => w.write_resize(t, c, r),
                    CaptureMsg::Input(t, source, data) => w.write_input(t, &source, &data),
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

    #[inline]
    pub fn tap_input(&self, pty_id: &str, source: &str, data: &[u8]) {
        if !self.is_capturing() {
            return;
        }
        let map = self.map.lock().unwrap();
        if let Some(cap) = map.get(pty_id) {
            let t = cap.start.elapsed().as_secs_f64();
            let _ = cap
                .tx
                .send(CaptureMsg::Input(t, source.to_string(), data.to_vec()));
        }
    }
}

#[cfg(test)]
mod tests;
