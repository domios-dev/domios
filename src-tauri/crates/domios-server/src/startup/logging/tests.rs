use super::*;
use std::io::Write;

#[test]
fn callback_logs_never_include_url_credentials() {
    for url in [
        "domios://auth/callback?code=opaque-code&state=opaque-state",
        "domios://auth/callback",
        "domios%3A%2F%2Fauth%2Fcallback%3Fcode%3Dopaque-code%26state%3Dopaque-state",
        "http://127.0.0.1:43210/callback?state=opaque-state&code=opaque-code",
    ] {
        let message = format!("dependency received {url:?}");
        let redacted = redact_callback(&message);
        assert!(!redacted.contains(url));
        assert!(!redacted.contains("opaque-code"));
        assert!(!redacted.contains("opaque-state"));
    }
    assert_eq!(
        redact_callback("ordinary diagnostic"),
        "ordinary diagnostic"
    );
}

#[test]
fn the_total_log_budget_stays_within_two_hundred_megabytes() {
    let generations = (LOG_GENERATIONS + 1) as u64; // the live file plus its rotations
    assert!(
        generations * MAX_LOG_BYTES <= 200 * 1024 * 1024,
        "{generations} generations at {MAX_LOG_BYTES} bytes exceeds the 200 MB budget"
    );
}

#[test]
fn a_writer_past_the_cap_rotates_instead_of_growing() {
    let dir = tempfile::tempdir().unwrap();
    let live = dir.path().join("domios.log");
    std::fs::write(&live, vec![b'x'; (MAX_LOG_BYTES + 1) as usize]).unwrap();

    let mut sink = RotatingLog::open(dir.path()).unwrap();
    sink.write_all(b"after the cap\n").unwrap();
    sink.flush().unwrap();

    assert!(
        dir.path().join("domios.log.1").exists(),
        "the oversized generation must be kept as .1"
    );
    let live_len = std::fs::metadata(&live).unwrap().len();
    assert!(
        live_len < MAX_LOG_BYTES,
        "the live log restarts small after rotation, got {live_len} bytes"
    );
}

#[test]
fn rotation_drops_the_oldest_generation_rather_than_accumulating() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("domios.log"), b"live").unwrap();
    for generation in 1..=LOG_GENERATIONS {
        std::fs::write(
            dir.path().join(format!("domios.log.{generation}")),
            format!("gen{generation}"),
        )
        .unwrap();
    }

    rotate(dir.path());

    let extra = dir
        .path()
        .join(format!("domios.log.{}", LOG_GENERATIONS + 1));
    assert!(!extra.exists(), "rotation must not create a new generation");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("domios.log.1")).unwrap(),
        "live",
        "the live log becomes the newest generation"
    );
}

#[test]
fn a_writer_under_the_cap_never_rotates() {
    let dir = tempfile::tempdir().unwrap();
    let mut sink = RotatingLog::open(dir.path()).unwrap();
    sink.write_all(b"one line\n").unwrap();
    sink.flush().unwrap();
    assert!(!dir.path().join("domios.log.1").exists());
}

#[test]
fn log_composition_filters_the_actual_third_party_targets() {
    assert!(THIRD_PARTY_WARN_TARGETS.contains(&"hyper_util"));
    assert!(!THIRD_PARTY_WARN_TARGETS.contains(&"hyper"));
    assert!(THIRD_PARTY_WARN_TARGETS.contains(&"reqwest"));
    assert!(THIRD_PARTY_WARN_TARGETS.contains(&"tungstenite"));
    assert!(THIRD_PARTY_WARN_TARGETS.contains(&"tokio"));
    assert!(THIRD_PARTY_WARN_TARGETS.contains(&"mdns_sd"));
}
