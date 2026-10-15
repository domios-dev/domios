use std::io::{BufRead, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{{\"ready\":true}}")?;
    stdout.flush()?;
    for line in stdin.lock().lines() {
        let request: serde_json::Value = serde_json::from_str(&line?)?;
        let text = request["text"].as_str().ok_or("missing text")?;
        let started = std::time::Instant::now();
        let response = match codeterm_mem::embeddings::embed(text) {
            Ok(vector) => {
                serde_json::json!({"embedding": vector, "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0})
            }
            Err(error) => serde_json::json!({"error": error}),
        };
        writeln!(stdout, "{response}")?;
        stdout.flush()?;
    }
    Ok(())
}
