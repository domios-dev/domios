//! Embedding worker subprocess.
//!
//! Isolates the fastembed/ort ONNX runtime in a child process so its
//! known Windows memory leak can be bounded by periodic child restart.
//!
//! Protocol: line-delimited JSON over stdin/stdout.
//! Request:  `{"id": "...", "text": "..."}`
//! Response: `{"id": "...", "embedding": [f32; 384]}` or `{"id": "...", "error": "..."}`
//!
//! Each line is one request. Child flushes stdout after every response.
//! Parent is responsible for restarting the child when memory grows past
//! a threshold (or after N requests).

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};

#[derive(Deserialize)]
struct Request {
    id: String,
    text: String,
}

#[derive(Serialize)]
struct Response {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    embedding: Option<Vec<f32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[cfg(feature = "embeddings")]
fn main() {
    use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

    let mut model = match TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::ParaphraseMLMiniLML12V2),
    ) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[codeterm-embed] model init failed: {}", e);
            std::process::exit(1);
        }
    };

    eprintln!("[codeterm-embed] ready");

    let stdin = std::io::stdin();
    let reader = BufReader::new(stdin.lock());
    let mut stdout = std::io::stdout();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.is_empty() { continue; }

        let req: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let resp = Response { id: String::new(), embedding: None, error: Some(format!("parse: {}", e)) };
                let _ = writeln!(stdout, "{}", serde_json::to_string(&resp).unwrap_or_default());
                let _ = stdout.flush();
                continue;
            }
        };

        let resp = match model.embed(vec![req.text], None) {
            Ok(mut embs) => Response {
                id: req.id,
                embedding: embs.pop(),
                error: None,
            },
            Err(e) => Response {
                id: req.id,
                embedding: None,
                error: Some(format!("embed: {}", e)),
            },
        };

        match serde_json::to_string(&resp) {
            Ok(json) => {
                let _ = writeln!(stdout, "{}", json);
                let _ = stdout.flush();
            }
            Err(e) => {
                eprintln!("[codeterm-embed] serialize failed: {}", e);
            }
        }
    }
}

#[cfg(not(feature = "embeddings"))]
fn main() {
    eprintln!("[codeterm-embed] built without embeddings feature");
    std::process::exit(1);
}
