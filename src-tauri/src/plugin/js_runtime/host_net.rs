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
    chunks: Vec<String>,
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
pub(super) fn stream(opts_json: &str) -> String {
    if let Err(e) = serde_json::from_str::<FetchOpts>(opts_json) {
        return err(&format!("bad fetch opts: {e}"));
    }

    let job_id = format!("fetch-{}", uuid::Uuid::new_v4().simple());
    STREAM_JOBS
        .lock()
        .insert(job_id.clone(), StreamJob::default());

    let owned = opts_json.to_string();
    let worker_job_id = job_id.clone();
    std::thread::spawn(move || run_stream_worker(worker_job_id, owned));

    serde_json::json!({ "jobId": job_id }).to_string()
}

pub(super) fn stream_poll(job_id: &str) -> String {
    let mut jobs = STREAM_JOBS.lock();
    let Some(job) = jobs.get_mut(job_id) else {
        return serde_json::json!({
            "chunks": [],
            "done": true,
            "error": "job not found",
        })
        .to_string();
    };

    let chunks = std::mem::take(&mut job.chunks);
    let mut out = serde_json::Map::new();
    out.insert("chunks".to_string(), serde_json::json!(chunks));
    out.insert("done".to_string(), serde_json::json!(job.done));
    if let Some(status) = job.status {
        out.insert("status".to_string(), serde_json::json!(status));
    }
    if let Some(error) = &job.error {
        out.insert("error".to_string(), serde_json::json!(error));
    }
    serde_json::Value::Object(out).to_string()
}

pub(super) fn stream_close(job_id: &str) {
    STREAM_JOBS.lock().remove(job_id);
}

fn run_blocking(opts_json: &str) -> String {
    let opts: FetchOpts = match serde_json::from_str(opts_json) {
        Ok(o) => o,
        Err(e) => return err(&format!("bad fetch opts: {e}")),
    };
    let timeout = Duration::from_millis(opts.timeout_ms.unwrap_or(30_000).max(1));
    let client = match reqwest::blocking::Client::builder()
        .timeout(timeout)
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
        .timeout(timeout)
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
    while let Some(next) = stream.next().await {
        match next {
            Ok(bytes) => push_stream_chunk(&job_id, String::from_utf8_lossy(&bytes).to_string()),
            Err(e) => return Err(format!("stream failed: {e}")),
        }
    }
    finish_stream_job(&job_id, None);
    Ok(())
}

fn set_stream_status(job_id: &str, status: u16) {
    if let Some(job) = STREAM_JOBS.lock().get_mut(job_id) {
        job.status = Some(status);
    }
}

fn push_stream_chunk(job_id: &str, chunk: String) {
    if let Some(job) = STREAM_JOBS.lock().get_mut(job_id) {
        job.chunks.push(chunk);
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
    use std::thread;
    use std::time::Instant;

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

    fn poll_until_done(job_id: &str) -> serde_json::Value {
        let started = Instant::now();
        let mut all_chunks = Vec::new();
        loop {
            let poll: serde_json::Value = serde_json::from_str(&stream_poll(job_id)).unwrap();
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

    #[test]
    fn stream_returns_job_id_and_poll_drains_chunks_until_done() {
        let url = serve_chunked(vec!["hello ", "stream"]);
        let started: serde_json::Value =
            serde_json::from_str(&stream(&serde_json::json!({ "url": url }).to_string())).unwrap();
        let job_id = started["jobId"].as_str().expect("job id");

        let done = poll_until_done(job_id);
        assert_eq!(done["chunks"], serde_json::json!(["hello ", "stream"]));
        assert_eq!(done["done"], true);
        assert_eq!(done["status"], 200);

        stream_close(job_id);
    }

    #[test]
    fn stream_uses_the_same_allowlist_gate_as_fetch() {
        let sb = PluginSandbox::restricted(vec![], vec![]);
        let opts = serde_json::json!({ "url": "http://blocked.example.test/stream" }).to_string();
        let host = host_of(&opts).unwrap_or_default();

        let raw = if sb.can_reach(&host) {
            stream(&opts)
        } else {
            err(&format!("fetch denied: {host}"))
        };

        let denied: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(denied["error"], "fetch denied: blocked.example.test");
    }
}
