use super::cancellation::Cancellation;
use std::sync::mpsc::Receiver;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub struct Receipt {
    label: String,
    started: SystemTime,
    elapsed: Instant,
    spawned: Receiver<u32>,
    cancellation: Cancellation,
}

pub struct Finished {
    pub child_pids: Vec<u32>,
    pub diagnostic: String,
}

impl Receipt {
    pub fn start(label: &str, cancellation: &Cancellation) -> Self {
        Self {
            label: format!(
                "{}:{label}",
                std::thread::current().name().unwrap_or("unnamed")
            ),
            started: SystemTime::now(),
            elapsed: Instant::now(),
            spawned: cancellation.observe_spawned(),
            cancellation: cancellation.clone(),
        }
    }

    pub fn finish(self, done: bool, outcome: &str, detail: &str) -> Finished {
        let ended = SystemTime::now();
        let child_pids = self.spawned.try_iter().collect::<Vec<_>>();
        let mut diagnostic = format!(
            "fixture={} child_pids={child_pids:?} start_unix_ms={} end_unix_ms={} elapsed_ms={} outcome={outcome}",
            self.label, timestamp(self.started), timestamp(ended), self.elapsed.elapsed().as_millis()
        );
        eprintln!("{diagnostic}");
        for captured in self.cancellation.finish_observation() {
            diagnostic.push_str("; ");
            diagnostic.push_str(&captured);
            if !done {
                eprintln!("{captured}");
            }
        }
        if !detail.is_empty() {
            diagnostic.push_str("; ");
            diagnostic.push_str(detail);
            if !done {
                eprintln!("{detail}");
            }
        }
        Finished {
            child_pids,
            diagnostic,
        }
    }
}

fn timestamp(at: SystemTime) -> String {
    at.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().to_string())
        .unwrap_or_else(|_| "before_epoch".into())
}

pub fn stderr_tail(stderr: &str) -> String {
    let redacted = codeterm_util::redact::text(stderr);
    let reversed = redacted.chars().rev().take(512).collect::<String>();
    reversed.chars().rev().collect()
}
