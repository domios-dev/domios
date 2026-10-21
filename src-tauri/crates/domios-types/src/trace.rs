use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{BufRead, Write};
use std::path::Path;

pub const MAX_BYTES: u64 = 5 * 1024 * 1024;
pub const GENERATIONS: usize = 3;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub ts: u64,
    pub trace_id: String,
    pub stage: String,
    pub tab: String,
    pub provider: String,
    pub outcome: String,
    pub detail: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related_id: Option<String>,
}

impl Event {
    pub fn new(id: &str, stage: &str, tab: &str, provider: &str, outcome: &str) -> Self {
        Self {
            ts: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            trace_id: id.into(),
            stage: stage.into(),
            tab: tab.into(),
            provider: provider.into(),
            outcome: outcome.into(),
            detail: String::new(),
            related_id: None,
        }
    }

    pub fn related(mut self, id: Option<&str>) -> Self {
        self.related_id = id.map(str::to_string);
        self
    }
}

pub struct Sink {
    dir: std::path::PathBuf,
    file: Option<std::fs::File>,
    written: u64,
    sources: std::collections::HashMap<String, String>,
}

impl Sink {
    pub fn open(dir: &Path, enabled: bool) -> std::io::Result<Self> {
        let mut sink = Self {
            dir: dir.into(),
            file: None,
            written: 0,
            sources: Default::default(),
        };
        if enabled {
            std::fs::create_dir_all(dir)?;
            sink.reopen()?;
        }
        Ok(sink)
    }

    fn reopen(&mut self) -> std::io::Result<()> {
        let path = self.dir.join("trace.jsonl");
        self.written = std::fs::metadata(&path).map_or(0, |m| m.len());
        self.file = Some(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?,
        );
        Ok(())
    }

    pub fn append(&mut self, event: &Event) -> std::io::Result<()> {
        if self.file.is_none() {
            return Ok(());
        }
        let mut event = event.clone();
        if event.stage == "trace.link" {
            if let Some(source) = event.related_id.as_ref() {
                if self.sources.len() >= 4096 {
                    self.sources.clear();
                }
                self.sources.insert(source.clone(), event.trace_id.clone());
            }
        } else if let Some(root) = self.sources.get(&event.trace_id) {
            event.related_id.get_or_insert(event.trace_id.clone());
            event.trace_id = root.clone();
        }
        event.detail = event.detail.chars().take(240).collect();
        let mut bytes = serde_json::to_vec(&event)?;
        bytes.push(b'\n');
        if bytes.len() as u64 > MAX_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "trace event exceeds the file budget",
            ));
        }
        if self.written + bytes.len() as u64 > MAX_BYTES {
            self.file.take();
            let _ = std::fs::remove_file(self.dir.join(format!("trace.jsonl.{GENERATIONS}")));
            for generation in (1..GENERATIONS).rev() {
                let _ = std::fs::rename(
                    self.dir.join(format!("trace.jsonl.{generation}")),
                    self.dir.join(format!("trace.jsonl.{}", generation + 1)),
                );
            }
            std::fs::rename(self.dir.join("trace.jsonl"), self.dir.join("trace.jsonl.1"))?;
            self.reopen()?;
        }
        if let Some(file) = self.file.as_mut() {
            file.write_all(&bytes)?;
            self.written += bytes.len() as u64;
        }
        Ok(())
    }
}

pub fn read(dir: &Path) -> std::io::Result<Vec<Event>> {
    let mut events = Vec::new();
    for generation in (0..=GENERATIONS).rev() {
        let name = if generation == 0 {
            "trace.jsonl".into()
        } else {
            format!("trace.jsonl.{generation}")
        };
        let file = match std::fs::File::open(dir.join(name)) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        for line in std::io::BufReader::new(file).lines() {
            if let Ok(event) = serde_json::from_str::<Event>(&line?) {
                events.push(event);
            }
        }
    }
    events.sort_by_key(|event| event.ts);
    Ok(canonicalize(&events))
}

fn canonicalize(events: &[Event]) -> Vec<Event> {
    let mut edges = std::collections::HashMap::<&str, Vec<&str>>::new();
    for event in events {
        edges.entry(&event.trace_id).or_default();
        if let Some(id) = event.related_id.as_deref() {
            edges.entry(&event.trace_id).or_default().push(id);
            edges.entry(id).or_default().push(&event.trace_id);
        }
    }
    let mut roots = std::collections::HashMap::new();
    // A send's existing identity wins even when its source delta arrived first.
    for event in events
        .iter()
        .filter(|event| event.stage == "send.started")
        .chain(events.iter())
    {
        let mut pending = vec![event.trace_id.as_str()];
        while let Some(id) = pending.pop() {
            if roots.contains_key(id) {
                continue;
            }
            roots.insert(id, event.trace_id.as_str());
            pending.extend(edges.get(id).into_iter().flatten().copied());
        }
    }
    events
        .iter()
        .map(|event| {
            let mut event = event.clone();
            if let Some(root) = roots.get(event.trace_id.as_str()) {
                if *root != event.trace_id {
                    event.related_id.get_or_insert(event.trace_id.clone());
                    event.trace_id = (*root).into();
                }
            }
            event
        })
        .collect()
}

pub fn select(events: &[Event], selector: &str, since: u64) -> Vec<Event> {
    let events = canonicalize(events);
    let mut ids = HashSet::from([selector.to_string()]);
    for event in events.iter().filter(|e| e.ts >= since && e.tab == selector) {
        ids.insert(event.trace_id.clone());
    }
    loop {
        let before = ids.len();
        for event in &events {
            if ids.contains(&event.trace_id)
                || event.related_id.as_ref().is_some_and(|id| ids.contains(id))
            {
                ids.insert(event.trace_id.clone());
                ids.extend(event.related_id.clone());
            }
        }
        if before == ids.len() {
            break;
        }
    }
    events
        .iter()
        .filter(|e| e.ts >= since && ids.contains(&e.trace_id))
        .cloned()
        .collect()
}

pub fn gaps(events: &[Event]) -> Vec<String> {
    let events = canonicalize(events);
    let mut gaps = Vec::new();
    let answered: HashSet<_> = events
        .iter()
        .filter(|event| event.stage == "transcript.assistant")
        .map(|event| event.trace_id.as_str())
        .collect();
    let stages: HashSet<_> = events
        .iter()
        .map(|event| {
            (
                event.related_id.as_deref().unwrap_or(&event.trace_id),
                event.stage.as_str(),
            )
        })
        .collect();
    for event in &events {
        if event.stage == "send.delivered" && !answered.contains(event.trace_id.as_str()) {
            gaps.push(format!("{}: send delivered to {} but no assistant transcript append in retained window (pending or missing)", event.trace_id, event.tab));
        }
        if event.stage == "trace.dropped" {
            gaps.push(format!("trace capture incomplete: {}", event.detail));
        }
        let expected = match event.stage.as_str() {
            "relay.expected" => "relay.registered",
            "relay.registered" => "relay.observed",
            "report.emitted" => "report.written",
            "send.started" => "send.delivered",
            _ => continue,
        };
        let record_id = event.related_id.as_deref().unwrap_or(&event.trace_id);
        if !stages.contains(&(record_id, expected)) {
            gaps.push(format!(
                "{}: {} by {} (record {}) but no {} in retained window",
                event.trace_id, event.stage, event.tab, record_id, expected
            ));
        }
    }
    gaps.sort();
    gaps.dedup();
    gaps
}

pub fn recent(events: &[Event], limit: usize) -> Vec<Event> {
    let mut related = std::collections::HashMap::<&str, Vec<&str>>::new();
    for event in events {
        related.entry(&event.trace_id).or_default();
        if let Some(id) = event.related_id.as_deref() {
            related.entry(&event.trace_id).or_default().push(id);
            related.entry(id).or_default().push(&event.trace_id);
        }
    }
    let mut roots = std::collections::HashMap::new();
    for event in events {
        if roots.contains_key(event.trace_id.as_str()) {
            continue;
        }
        let mut pending = vec![event.trace_id.as_str()];
        while let Some(id) = pending.pop() {
            if roots.contains_key(id) {
                continue;
            }
            roots.insert(id, event.trace_id.as_str());
            pending.extend(related.get(id).into_iter().flatten().copied());
        }
    }
    let mut seen = HashSet::new();
    events
        .iter()
        .rev()
        .filter_map(|event| {
            let root = roots
                .get(event.trace_id.as_str())
                .copied()
                .unwrap_or(&event.trace_id);
            if !seen.insert(root) {
                return None;
            }
            let mut event = event.clone();
            event.trace_id = root.into();
            Some(event)
        })
        .take(limit)
        .collect()
}

#[cfg(test)]
mod tests;
