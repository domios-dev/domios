use std::io::Write;
use std::time::Instant;

use super::*;

const FIXTURE_BYTES: u64 = 250 * 1024 * 1024;
const REOPENS: usize = 20;
const APPENDED_RECORDS: usize = 32;
const SCROLL_PAGES: usize = 5;

fn record(index: usize, pad: &str) -> String {
    if index % 2 == 0 {
        serde_json::json!({
            "type": "user",
            "uuid": format!("u-{index}"),
            "timestamp": "2026-10-21T00:00:00Z",
            "message": {"role": "user", "content": format!("question {index} {pad}")},
        })
    } else {
        serde_json::json!({
            "type": "assistant",
            "uuid": format!("a-{index}"),
            "timestamp": "2026-10-21T00:00:01Z",
            "message": {
                "id": format!("msg-{index}"),
                "role": "assistant",
                "content": [{"type": "text", "text": format!("answer {index} {pad}")}],
            },
        })
    }
    .to_string()
        + "\n"
}

fn append_records(path: &Path, from: usize, count: usize, pad: &str) -> u64 {
    let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    let mut written = 0;
    for index in from..from + count {
        let line = record(index, pad);
        written += line.len() as u64;
        file.write_all(line.as_bytes()).unwrap();
    }
    written
}

struct Cost {
    step: &'static str,
    ms: f64,
    bytes: u64,
}

fn timed(step: &'static str, run: impl FnOnce() -> u64) -> Cost {
    let started = Instant::now();
    let bytes = run();
    Cost {
        step,
        ms: started.elapsed().as_secs_f64() * 1000.0,
        bytes,
    }
}

fn scroll_back(
    registry: &Arc<AgentRegistry>,
    provider_id: &str,
    path: &Path,
    mut until: u64,
) -> u64 {
    let mut bytes = 0;
    for _ in 0..SCROLL_PAGES {
        let page = session_window::parse_page_before(registry, provider_id, path, until).unwrap();
        assert!(page.end_offset - page.start_offset <= session_window::BACKFILL_PAGE_BYTES);
        assert!(!page.messages.is_empty());
        bytes += page.end_offset - page.start_offset;
        until = page.start_offset;
    }
    bytes
}

/// Opt-in: generates a 250 MB synthetic transcript and prints open,
/// re-open, append and scroll-back costs for the uncached and cached paths.
#[test]
#[ignore = "generates a 250 MB fixture; run explicitly with --ignored"]
fn measure_250mb_transcript_open_reopen_append_scroll() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("synthetic-session.jsonl");
    let pad = "x".repeat(1800);
    let mut next = 0;
    {
        let mut file = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
        let mut written = 0;
        while written < FIXTURE_BYTES {
            let line = record(next, &pad);
            written += line.len() as u64;
            file.write_all(line.as_bytes()).unwrap();
            next += 1;
        }
        file.flush().unwrap();
    }
    let total = std::fs::metadata(&path).unwrap().len();
    let registry = Arc::new(AgentRegistry::new());
    let mut plugins = crate::plugin::PluginManager::new();
    plugins.set_registrar(registry.clone());
    plugins.activate_builtin_native_plugins().unwrap();
    let ids = registry.provider_ids();
    assert_eq!(
        ids.len(),
        1,
        "measurement requires one native transcript provider"
    );
    let provider_id = ids[0].as_str();

    let mut before = Vec::new();
    let opened = session_window::snapshot(&registry, provider_id, &path);
    before.push(timed("open", || {
        session_window::snapshot(&registry, provider_id, &path).bytes_read
    }));
    assert!(!opened.messages.is_empty());
    before.push(timed("re-open x20", || {
        (0..REOPENS)
            .map(|_| session_window::snapshot(&registry, provider_id, &path).bytes_read)
            .sum()
    }));
    let appended = append_records(&path, next, APPENDED_RECORDS, &pad);
    next += APPENDED_RECORDS;
    before.push(timed("append+open", || {
        session_window::snapshot(&registry, provider_id, &path).bytes_read
    }));
    let start = session_window::snapshot(&registry, provider_id, &path).start_offset;
    before.push(timed("scroll-back x5", || {
        scroll_back(&registry, provider_id, &path, start)
    }));

    let cache = TailCache::default();
    let key = TailKey {
        provider_id: provider_id.into(),
        path: path.clone(),
    };
    let port = JsonlTailPort {
        registry: &registry,
        provider_id,
        path: &path,
    };
    let mut after = Vec::new();
    after.push(timed("open", || {
        cache.read(&key, &port).unwrap().bytes_read
    }));
    after.push(timed("re-open x20", || {
        (0..REOPENS)
            .map(|_| {
                let read = cache.read(&key, &port).unwrap();
                assert_eq!(read.plan, TailPlan::Reuse);
                read.bytes_read
            })
            .sum()
    }));
    let appended_after = append_records(&path, next, APPENDED_RECORDS, &pad);
    let mut window = None;
    after.push(timed("append+open", || {
        let read = cache.read(&key, &port).unwrap();
        assert!(matches!(read.plan, TailPlan::Append { .. }));
        window = Some(read.rows.clone());
        read.bytes_read
    }));
    let window = window.unwrap();
    after.push(timed("scroll-back x5", || {
        scroll_back(&registry, provider_id, &path, window.start_offset)
    }));

    let fresh = session_window::snapshot(&registry, provider_id, &path);
    let ids = |messages: &[ChatMsg]| -> Vec<String> {
        messages.iter().map(|message| message.id.clone()).collect()
    };
    assert_eq!(ids(&window.messages), ids(&fresh.messages));
    assert!(after[1].bytes <= PROBE_BYTES * REOPENS as u64);
    assert!(after[2].bytes <= appended_after + 2 * PROBE_BYTES);
    assert!(before[1].bytes >= SNAPSHOT_WINDOW_BYTES / 2 * REOPENS as u64);

    let mut report = format!(
        "fixture_bytes={total} appended_bytes={appended}/{appended_after}\n{:<16}{:>14}{:>12}{:>14}{:>12}\n",
        "step", "before_bytes", "before_ms", "after_bytes", "after_ms"
    );
    for (old, new) in before.iter().zip(&after) {
        report += &format!(
            "{:<16}{:>14}{:>12.2}{:>14}{:>12.2}\n",
            old.step, old.bytes, old.ms, new.bytes, new.ms
        );
    }
    println!("{report}");
    if let Ok(target) = std::env::var("CODETERM_TAIL_BENCH_REPORT") {
        std::fs::write(target, &report).unwrap();
    }
}
