use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::Value;

#[derive(Debug, Clone)]
pub(crate) struct Recorded {
    pub path: String,
    pub authorization: Option<String>,
    pub body: Value,
}

type Respond = dyn Fn(&Value) -> (u16, String) + Send + Sync;

/// A loopback-only HTTP server standing in for the embeddings API.
pub(crate) struct MockServer {
    pub url: String,
    pub requests: Arc<Mutex<Vec<Recorded>>>,
    stop: Arc<AtomicBool>,
    address: std::net::SocketAddr,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl MockServer {
    pub fn start(respond: impl Fn(&Value) -> (u16, String) + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let respond: Arc<Respond> = Arc::new(respond);
        let thread = {
            let (requests, stop) = (requests.clone(), stop.clone());
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        return;
                    }
                    if let Ok(stream) = stream {
                        serve(stream, &requests, respond.as_ref());
                    }
                }
            })
        };
        Self {
            url: format!("http://{address}"),
            requests,
            stop,
            address,
            thread: Some(thread),
        }
    }

    /// Answer every request with vectors of `dimension`, the first component
    /// carrying the input's index so ordering is observable.
    pub fn embeddings(dimension: usize) -> Self {
        Self::start(move |body| (200, embeddings_response(body, dimension)))
    }

    pub fn status(status: u16) -> Self {
        Self::start(move |_| {
            (
                status,
                serde_json::json!({ "error": { "message": format!("mock {status}") } }).to_string(),
            )
        })
    }

    pub fn count(&self) -> usize {
        self.requests.lock().len()
    }

    pub fn last(&self) -> Recorded {
        self.requests
            .lock()
            .last()
            .cloned()
            .expect("no request recorded")
    }
}

pub(crate) fn embeddings_response(body: &Value, dimension: usize) -> String {
    let inputs = body["input"].as_array().map_or(0, Vec::len);
    let data: Vec<Value> = (0..inputs)
        .rev()
        .map(|index| {
            let mut vector = vec![0.0f32; dimension];
            vector[0] = index as f32 + 1.0;
            serde_json::json!({ "object": "embedding", "index": index, "embedding": vector })
        })
        .collect();
    serde_json::json!({ "object": "list", "data": data }).to_string()
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.address);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(stream: TcpStream, requests: &Mutex<Vec<Recorded>>, respond: &Respond) {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
        return;
    }
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or_default()
        .to_string();
    let (mut length, mut authorization) = (0usize, None);
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            let value = value.trim().to_string();
            match name.to_ascii_lowercase().as_str() {
                "content-length" => length = value.parse().unwrap_or(0),
                "authorization" => authorization = Some(value),
                _ => {}
            }
        }
    }
    let mut body = vec![0u8; length];
    if reader.read_exact(&mut body).is_err() {
        return;
    }
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let (status, reply) = respond(&body);
    requests.lock().push(Recorded {
        path,
        authorization,
        body,
    });
    let response = format!(
        "HTTP/1.1 {status} Mock\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
        reply.len()
    );
    let mut stream = reader.into_inner();
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}
