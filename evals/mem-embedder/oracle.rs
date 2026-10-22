use std::io::{BufRead, Write};
use std::sync::Arc;
use std::time::Instant;

use codeterm_mem::embedder::{local, ActiveEmbedder};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let model = std::env::args().nth(1).ok_or("expected local model id")?;
    let row = local::find(&model).ok_or("unknown local model")?;
    let active = ActiveEmbedder::global();
    active.replace(Arc::new(local::LocalProvider::new(row)));
    let mut stdout = std::io::stdout().lock();
    writeln!(
        stdout,
        "{}",
        serde_json::json!({"ready": true, "identity": active.identity()})
    )?;
    stdout.flush()?;
    for line in std::io::stdin().lock().lines() {
        let request: serde_json::Value = serde_json::from_str(&line?)?;
        let text = request["text"].as_str().ok_or("missing text")?;
        let started = Instant::now();
        let result = if request["query"].as_bool().unwrap_or(false) {
            active.query(text).ok_or("query unavailable".to_string())
        } else {
            active.passage(text).map_err(|error| error.to_string())
        };
        let response = match result {
            Ok(stamped) => {
                serde_json::json!({"embedding": stamped.vector, "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0})
            }
            Err(error) => serde_json::json!({"error": error}),
        };
        writeln!(stdout, "{response}")?;
        stdout.flush()?;
    }
    Ok(())
}
