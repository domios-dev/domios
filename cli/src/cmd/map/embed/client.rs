use super::path::resolve_embed_path;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

const CHILD_MAX_CALLS: u64 = 200;

#[derive(Serialize)]
struct Request<'a> {
    id: &'a str,
    text: &'a str,
}

#[derive(Deserialize)]
struct Response {
    #[allow(dead_code)]
    id: String,
    embedding: Option<Vec<f32>>,
    error: Option<String>,
}

struct Process {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    calls: u64,
}

impl Process {
    fn spawn() -> Result<Self, String> {
        let exe = resolve_embed_path()?;
        let mut cmd = Command::new(&exe);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("spawn {}: {}", exe.display(), e))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("no stdout")?);
        Ok(Self { child, stdin, stdout, calls: 0 })
    }

    fn request(&mut self, text: &str) -> Result<Vec<f32>, String> {
        let line = serde_json::to_string(&Request { id: "", text })
            .map_err(|e| e.to_string())?;
        writeln!(self.stdin, "{}", line).map_err(|e| format!("write: {}", e))?;
        self.stdin.flush().map_err(|e| format!("flush: {}", e))?;

        let mut resp = String::new();
        self.stdout.read_line(&mut resp).map_err(|e| format!("read: {}", e))?;
        if resp.is_empty() {
            return Err("child closed stdout".into());
        }
        let parsed: Response = serde_json::from_str(resp.trim())
            .map_err(|e| format!("parse: {}", e))?;
        self.calls += 1;
        if let Some(err) = parsed.error {
            return Err(err);
        }
        parsed.embedding.ok_or_else(|| "no embedding".into())
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct EmbedClient {
    proc: Option<Process>,
}

impl EmbedClient {
    pub fn new() -> Self {
        Self { proc: None }
    }

    pub fn embed(&mut self, text: &str) -> Result<Vec<f32>, String> {
        let normalized = preprocess(text);
        if let Some(p) = self.proc.as_ref() {
            if p.calls >= CHILD_MAX_CALLS {
                self.proc = None;
            }
        }
        if self.proc.is_none() {
            self.proc = Some(Process::spawn()?);
        }
        let p = self.proc.as_mut().unwrap();
        match p.request(&normalized) {
            Ok(v) => Ok(v),
            Err(e) => {
                self.proc = None;
                Err(e)
            }
        }
    }
}

fn preprocess(text: &str) -> String {
    let lower = text.to_lowercase();
    lower.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn vec_to_le_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vec_bytes_round_trip() {
        let v = vec![1.0_f32, -2.5, 3.25];
        let b = vec_to_le_bytes(&v);
        assert_eq!(b.len(), 12);
        let back: Vec<f32> = b
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        assert_eq!(back, v);
    }

    #[test]
    fn preprocess_lowercases_and_collapses() {
        assert_eq!(preprocess("Auth   FLOW\nhandler"), "auth flow handler");
    }
}
