//! `host.fetch` backend (host_api v2). Performs an allowlisted HTTP request and
//! returns `{status, headers, body}` as JSON. Host allowlist (`network.allow`)
//! is enforced by the caller via `PluginSandbox::can_reach`.
//!
//! Uses `reqwest::blocking`, which runs on its own internal thread pool, so it
//! is safe to call synchronously from a VM call even when a tokio runtime is
//! active on the calling thread.

use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Duration;

use futures_util::StreamExt;
use parking_lot::Mutex;
use serde::Deserialize;

const MAX_STREAM_JOBS: usize = 128;
const MAX_STREAM_JOB_BUFFERED_BYTES: usize = 4 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FetchOpts {
    url: String,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Default)]
struct StreamJob {
    plugin_id: String,
    chunks: Vec<String>,
    buffered_bytes: usize,
    done: bool,
    status: Option<u16>,
    error: Option<String>,
}

static STREAM_JOBS: LazyLock<Mutex<HashMap<String, StreamJob>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn err(msg: &str) -> String {
    serde_json::json!({ "error": msg }).to_string()
}

/// `host[:port]` of a URL, for the allowlist check. `None` if unparseable.
pub(super) fn host_of(opts_json: &str) -> Option<String> {
    let url = serde_json::from_str::<serde_json::Value>(opts_json)
        .ok()?
        .get("url")?
        .as_str()?
        .to_string();
    let rest = url.split("://").nth(1).unwrap_or(&url);
    rest.split('/').next().map(|s| s.to_string())
}

/// Perform the request described by `opts_json`. The host is already allowlisted.
///
/// Runs on a dedicated OS thread: `reqwest::blocking` panics if constructed while
/// a tokio runtime is active on the calling thread, and plugin VM calls happen on
/// tokio workers. The thread isolates the blocking client from that runtime; the
/// caller blocks on `join`, which is fine — the VM call is synchronous anyway.
pub(super) fn run(opts_json: &str) -> String {
    let owned = opts_json.to_string();
    match std::thread::spawn(move || run_blocking(&owned)).join() {
        Ok(s) => s,
        Err(_) => err("fetch worker panicked"),
    }
}

/// Start an HTTP stream. The caller must run the same allowlist gate as `run`.
pub(super) fn stream(plugin_id: &str, opts_json: &str) -> String {
    if let Err(e) = serde_json::from_str::<FetchOpts>(opts_json) {
        return err(&format!("bad fetch opts: {e}"));
    }

    let job_id = match start_stream_job(plugin_id) {
        Ok(job_id) => job_id,
        Err(e) => return err(&e),
    };

    let owned = opts_json.to_string();
    let worker_job_id = job_id.clone();
    std::thread::spawn(move || run_stream_worker(worker_job_id, owned));

    serde_json::json!({ "jobId": job_id }).to_string()
}

pub(super) fn stream_poll(plugin_id: &str, job_id: &str) -> String {
    let response = {
        let plugin_id = normalize_plugin_id(plugin_id);
        let mut jobs = STREAM_JOBS.lock();
        let Some(job) = jobs
            .get_mut(job_id)
            .filter(|job| job.plugin_id == plugin_id)
        else {
            return serde_json::json!({
                "chunks": [],
                "done": true,
                "error": "job not found",
            })
            .to_string();
        };

        let chunks = std::mem::take(&mut job.chunks);
        job.buffered_bytes = 0;
        let response = StreamPollResponse {
            chunks,
            done: job.done,
            status: job.status,
            error: job.error.clone(),
        };
        if job.done {
            jobs.remove(job_id);
        }
        response
    };

    let mut out = serde_json::Map::new();
    out.insert("chunks".to_string(), serde_json::json!(response.chunks));
    out.insert("done".to_string(), serde_json::json!(response.done));
    if let Some(status) = response.status {
        out.insert("status".to_string(), serde_json::json!(status));
    }
    if let Some(error) = response.error {
        out.insert("error".to_string(), serde_json::json!(error));
    }
    serde_json::Value::Object(out).to_string()
}

pub(super) fn stream_close(plugin_id: &str, job_id: &str) {
    let plugin_id = normalize_plugin_id(plugin_id);
    STREAM_JOBS
        .lock()
        .retain(|id, job| id != job_id || job.plugin_id != plugin_id);
}

pub(super) fn drop_plugin_jobs(plugin_id: &str) {
    let plugin_id = normalize_plugin_id(plugin_id);
    STREAM_JOBS
        .lock()
        .retain(|_, job| job.plugin_id != plugin_id);
}

fn run_blocking(opts_json: &str) -> String {
    let opts: FetchOpts = match serde_json::from_str(opts_json) {
        Ok(o) => o,
        Err(e) => return err(&format!("bad fetch opts: {e}")),
    };
    let timeout = Duration::from_millis(opts.timeout_ms.unwrap_or(30_000).max(1));
    let client = match reqwest::blocking::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(c) => c,
        Err(e) => return err(&format!("client: {e}")),
    };
    let method = reqwest::Method::from_bytes(
        opts.method
            .as_deref()
            .unwrap_or("GET")
            .to_uppercase()
            .as_bytes(),
    )
    .unwrap_or(reqwest::Method::GET);
    let mut req = client.request(method, &opts.url);
    for (k, v) in &opts.headers {
        req = req.header(k, v);
    }
    if let Some(b) = &opts.body {
        req = req.body(b.clone());
    }
    match req.send() {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let headers: HashMap<String, String> = resp
                .headers()
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                .collect();
            let body = resp.text().unwrap_or_default();
            serde_json::json!({ "status": status, "headers": headers, "body": body }).to_string()
        }
        Err(e) => err(&format!("request failed: {e}")),
    }
}

fn run_stream_worker(job_id: String, opts_json: String) {
    let result = std::panic::catch_unwind(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("stream runtime: {e}"))?;
        runtime.block_on(run_stream_async(job_id.clone(), opts_json))
    });

    let error = match result {
        Ok(Ok(())) => None,
        Ok(Err(e)) => Some(e),
        Err(_) => Some("fetch stream worker panicked".to_string()),
    };
    if let Some(error) = error {
        finish_stream_job(&job_id, Some(error));
    }
}

async fn run_stream_async(job_id: String, opts_json: String) -> Result<(), String> {
    let opts: FetchOpts =
        serde_json::from_str(&opts_json).map_err(|e| format!("bad fetch opts: {e}"))?;
    let timeout = Duration::from_millis(opts.timeout_ms.unwrap_or(30_000).max(1));
    let client = reqwest::Client::builder()
        .connect_timeout(timeout)
        .read_timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| format!("client: {e}"))?;
    let method = reqwest::Method::from_bytes(
        opts.method
            .as_deref()
            .unwrap_or("GET")
            .to_uppercase()
            .as_bytes(),
    )
    .unwrap_or(reqwest::Method::GET);
    let mut req = client.request(method, &opts.url);
    for (k, v) in &opts.headers {
        req = req.header(k, v);
    }
    if let Some(b) = &opts.body {
        req = req.body(b.clone());
    }

    let resp = req
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    set_stream_status(&job_id, resp.status().as_u16());

    let mut stream = resp.bytes_stream();
    let mut pending_utf8 = Vec::new();
    while let Some(next) = stream.next().await {
        match next {
            Ok(bytes) => {
                if let Some(chunk) = decode_utf8_prefix(&mut pending_utf8, &bytes) {
                    if !push_stream_chunk(&job_id, chunk) {
                        return Ok(());
                    }
                }
            }
            Err(e) => return Err(format!("stream failed: {e}")),
        }
    }
    if let Some(chunk) = flush_utf8_remainder(&mut pending_utf8) {
        if !push_stream_chunk(&job_id, chunk) {
            return Ok(());
        }
    }
    finish_stream_job(&job_id, None);
    Ok(())
}

struct StreamPollResponse {
    chunks: Vec<String>,
    done: bool,
    status: Option<u16>,
    error: Option<String>,
}

fn normalize_plugin_id(plugin_id: &str) -> String {
    if plugin_id.is_empty() {
        "anonymous".to_string()
    } else {
        plugin_id.to_string()
    }
}

fn start_stream_job(plugin_id: &str) -> Result<String, String> {
    let plugin_id = normalize_plugin_id(plugin_id);
    let mut jobs = STREAM_JOBS.lock();
    jobs.retain(|_, job| !job.done || !job.chunks.is_empty());
    if jobs.len() >= MAX_STREAM_JOBS {
        return Err("too many fetch stream jobs".to_string());
    }

    let job_id = format!("fetch:{plugin_id}:{}", uuid::Uuid::new_v4().simple());
    jobs.insert(
        job_id.clone(),
        StreamJob {
            plugin_id,
            ..StreamJob::default()
        },
    );
    Ok(job_id)
}

fn decode_utf8_prefix(pending: &mut Vec<u8>, frame: &[u8]) -> Option<String> {
    pending.extend_from_slice(frame);
    match std::str::from_utf8(pending) {
        Ok(valid) => {
            let out = valid.to_string();
            pending.clear();
            (!out.is_empty()).then_some(out)
        }
        Err(e) if e.error_len().is_none() => {
            let valid_up_to = e.valid_up_to();
            if valid_up_to == 0 {
                return None;
            }
            let out = std::str::from_utf8(&pending[..valid_up_to])
                .unwrap_or_default()
                .to_string();
            let tail = pending[valid_up_to..].to_vec();
            *pending = tail;
            Some(out)
        }
        Err(_) => {
            let out = String::from_utf8_lossy(pending).to_string();
            pending.clear();
            (!out.is_empty()).then_some(out)
        }
    }
}

fn flush_utf8_remainder(pending: &mut Vec<u8>) -> Option<String> {
    if pending.is_empty() {
        None
    } else {
        let out = String::from_utf8_lossy(pending).to_string();
        pending.clear();
        Some(out)
    }
}

fn set_stream_status(job_id: &str, status: u16) {
    if let Some(job) = STREAM_JOBS.lock().get_mut(job_id) {
        job.status = Some(status);
    }
}

fn push_stream_chunk(job_id: &str, chunk: String) -> bool {
    if let Some(job) = STREAM_JOBS.lock().get_mut(job_id) {
        if job.done {
            return false;
        }
        let chunk_bytes = chunk.len();
        if job.buffered_bytes.saturating_add(chunk_bytes) > MAX_STREAM_JOB_BUFFERED_BYTES {
            job.done = true;
            job.error = Some("fetch stream buffer limit exceeded".to_string());
            return false;
        }
        job.buffered_bytes += chunk_bytes;
        job.chunks.push(chunk);
        true
    } else {
        false
    }
}

fn finish_stream_job(job_id: &str, error: Option<String>) {
    if let Some(job) = STREAM_JOBS.lock().get_mut(job_id) {
        job.done = true;
        job.error = error;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::js_runtime::host_api::PluginSandbox;
    use std::io::Write;
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Instant;

    static TEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn serve_chunked(chunks: Vec<&'static str>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut request);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: text/plain\r\n\r\n"
            )
            .unwrap();
            for chunk in chunks {
                write!(stream, "{:x}\r\n{}\r\n", chunk.len(), chunk).unwrap();
                stream.flush().unwrap();
                thread::sleep(Duration::from_millis(10));
            }
            write!(stream, "0\r\n\r\n").unwrap();
        });
        format!("http://{addr}/stream")
    }

    fn poll_until_done(plugin_id: &str, job_id: &str) -> serde_json::Value {
        let started = Instant::now();
        let mut all_chunks = Vec::new();
        loop {
            let poll: serde_json::Value =
                serde_json::from_str(&stream_poll(plugin_id, job_id)).unwrap();
            if let Some(chunks) = poll["chunks"].as_array() {
                all_chunks.extend(chunks.iter().filter_map(|c| c.as_str()).map(str::to_string));
            }
            if poll["done"].as_bool() == Some(true) {
                return serde_json::json!({
                    "chunks": all_chunks,
                    "done": true,
                    "status": poll["status"],
                    "error": poll["error"],
                });
            }
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "stream timed out"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn reset_stream_jobs() {
        STREAM_JOBS.lock().clear();
    }

    fn serve_redirect_pair() -> (String, mpsc::Receiver<()>) {
        let target = TcpListener::bind("127.0.0.1:0").unwrap();
        let target_addr = target.local_addr().unwrap();
        let redirect = TcpListener::bind("127.0.0.1:0").unwrap();
        let redirect_addr = redirect.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            if let Ok((mut stream, _)) = target.accept() {
                let _ = tx.send(());
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: 7\r\n\r\nescaped"
                );
            }
        });
        thread::spawn(move || {
            let (mut stream, _) = redirect.accept().unwrap();
            let mut request = [0_u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut request);
            write!(
                stream,
                "HTTP/1.1 302 Found\r\nLocation: http://{target_addr}/escaped\r\nContent-Length: 0\r\n\r\n"
            )
            .unwrap();
        });

        (format!("http://{redirect_addr}/start"), rx)
    }

    #[test]
    fn stream_returns_job_id_and_poll_drains_chunks_until_done() {
        let _guard = TEST_LOCK.lock();
        reset_stream_jobs();
        let url = serve_chunked(vec!["hello ", "stream"]);
        let started: serde_json::Value = serde_json::from_str(&stream(
            "test-plugin",
            &serde_json::json!({ "url": url }).to_string(),
        ))
        .unwrap();
        let job_id = started["jobId"].as_str().expect("job id");
        assert!(job_id.starts_with("fetch:test-plugin:"));

        let done = poll_until_done("test-plugin", job_id);
        assert_eq!(done["chunks"], serde_json::json!(["hello ", "stream"]));
        assert_eq!(done["done"], true);
        assert_eq!(done["status"], 200);

        let missing: serde_json::Value =
            serde_json::from_str(&stream_poll("test-plugin", job_id)).unwrap();
        assert_eq!(missing["error"], "job not found");
    }

    #[test]
    fn stream_uses_the_same_allowlist_gate_as_fetch() {
        let _guard = TEST_LOCK.lock();
        reset_stream_jobs();
        let sb = PluginSandbox::restricted(vec![], vec![]);
        let opts = serde_json::json!({ "url": "http://blocked.example.test/stream" }).to_string();
        let host = host_of(&opts).unwrap_or_default();

        let raw = if sb.can_reach(&host) {
            stream("test-plugin", &opts)
        } else {
            err(&format!("fetch denied: {host}"))
        };

        let denied: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(denied["error"], "fetch denied: blocked.example.test");
    }

    #[test]
    fn utf8_decoder_carries_multibyte_boundaries_without_replacement_chars() {
        let _guard = TEST_LOCK.lock();
        let frames = [vec![0xd0], vec![0x96, 0xf0], vec![0x9f, 0x99], vec![0x82]];
        let mut pending = Vec::new();
        let mut out = String::new();

        for frame in frames {
            if let Some(chunk) = decode_utf8_prefix(&mut pending, &frame) {
                out.push_str(&chunk);
            }
        }
        if let Some(chunk) = flush_utf8_remainder(&mut pending) {
            out.push_str(&chunk);
        }

        assert_eq!(out, "Ж🙂");
        assert!(!out.contains('\u{fffd}'));
    }

    #[test]
    fn stream_job_count_is_capped() {
        let _guard = TEST_LOCK.lock();
        reset_stream_jobs();
        for _ in 0..MAX_STREAM_JOBS {
            start_stream_job("cap-plugin").unwrap();
        }

        assert_eq!(
            start_stream_job("cap-plugin").unwrap_err(),
            "too many fetch stream jobs"
        );
        drop_plugin_jobs("cap-plugin");
    }

    #[test]
    fn stream_job_buffer_is_capped() {
        let _guard = TEST_LOCK.lock();
        reset_stream_jobs();
        let job_id = start_stream_job("cap-plugin").unwrap();
        assert!(!push_stream_chunk(
            &job_id,
            "x".repeat(MAX_STREAM_JOB_BUFFERED_BYTES + 1)
        ));

        let poll: serde_json::Value =
            serde_json::from_str(&stream_poll("cap-plugin", &job_id)).unwrap();
        assert_eq!(poll["done"], true);
        assert_eq!(poll["error"], "fetch stream buffer limit exceeded");
    }

    #[test]
    fn blocking_fetch_does_not_follow_redirects_outside_allowlist() {
        let _guard = TEST_LOCK.lock();
        let (url, escaped_rx) = serve_redirect_pair();
        let body: serde_json::Value =
            serde_json::from_str(&run(&serde_json::json!({ "url": url }).to_string())).unwrap();

        assert_eq!(body["status"], 302);
        assert!(escaped_rx.recv_timeout(Duration::from_millis(100)).is_err());
    }

    #[test]
    fn stream_fetch_does_not_follow_redirects_outside_allowlist() {
        let _guard = TEST_LOCK.lock();
        reset_stream_jobs();
        let (url, escaped_rx) = serve_redirect_pair();
        let started: serde_json::Value = serde_json::from_str(&stream(
            "redirect-plugin",
            &serde_json::json!({ "url": url }).to_string(),
        ))
        .unwrap();
        let job_id = started["jobId"].as_str().expect("job id");

        let done = poll_until_done("redirect-plugin", job_id);
        assert_eq!(done["status"], 302);
        assert!(escaped_rx.recv_timeout(Duration::from_millis(100)).is_err());
    }
}
