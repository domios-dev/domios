use super::*;
use std::io::Read;

/// Parse a `.cast` file into (header, events). Each event is (time, code, data-bytes).
fn read_cast(path: &Path) -> (serde_json::Value, Vec<(f64, String, Vec<u8>)>) {
    let mut s = String::new();
    std::fs::File::open(path)
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    let mut lines = s.lines();
    let header: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
    let events = lines
        .filter(|l| !l.is_empty())
        .map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            let t = v[0].as_f64().unwrap();
            let code = v[1].as_str().unwrap().to_string();
            let data = v[2].as_str().unwrap().as_bytes().to_vec();
            (t, code, data)
        })
        .collect();
    (header, events)
}

#[test]
fn header_records_geometry_and_timestamp() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("h.cast");
    CastWriter::create(&path, 120, 50, 1700000000)
        .unwrap()
        .finish()
        .unwrap();
    let (header, _) = read_cast(&path);
    assert_eq!(header["version"], 2);
    assert_eq!(header["width"], 120);
    assert_eq!(header["height"], 50);
    assert_eq!(header["timestamp"], 1700000000u64);
}

#[test]
fn output_round_trips_byte_identical_across_multibyte_chunk_split() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rt.cast");
    // ANSI escapes + a box-drawing char (╭ = E2 95 AD) deliberately split across
    // two write_output calls — the real PTY-read boundary case.
    let full = "\x1b[1mhi\x1b[0m ╭─╮\r\n".as_bytes().to_vec();
    let split = "\x1b[1mhi\x1b[0m ".len() + 2; // 2 bytes into the 3-byte ╭ (E2 95 AD)
    let mut w = CastWriter::create(&path, 80, 24, 0).unwrap();
    w.write_output(0.0, &full[..split]).unwrap();
    w.write_output(0.1, &full[split..]).unwrap();
    w.finish().unwrap();

    let (_, events) = read_cast(&path);
    let recovered: Vec<u8> = events
        .iter()
        .filter(|(_, code, _)| code == "o")
        .flat_map(|(_, _, data)| data.clone())
        .collect();
    assert_eq!(
        recovered, full,
        "concatenated output must equal input bytes"
    );
}

#[test]
fn timestamps_are_monotonic_and_start_near_zero() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ts.cast");
    let mut w = CastWriter::create(&path, 80, 24, 0).unwrap();
    w.write_output(0.0, b"a").unwrap();
    w.write_output(0.5, b"b").unwrap();
    w.write_output(1.25, b"c").unwrap();
    w.finish().unwrap();

    let (_, events) = read_cast(&path);
    let times: Vec<f64> = events.iter().map(|(t, _, _)| *t).collect();
    assert!(times[0] < 0.01, "first event starts near 0");
    for pair in times.windows(2) {
        assert!(pair[1] >= pair[0], "timestamps non-decreasing");
    }
}

#[test]
fn resize_emits_r_row_with_cols_x_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rz.cast");
    let mut w = CastWriter::create(&path, 80, 24, 0).unwrap();
    w.write_output(0.0, b"before").unwrap();
    w.write_resize(0.2, 100, 40).unwrap();
    w.finish().unwrap();

    let (_, events) = read_cast(&path);
    let resize = events.iter().find(|(_, code, _)| code == "r").unwrap();
    assert_eq!(resize.2, b"100x40");
}

#[test]
fn screen_txt_equals_vt100_render_of_final_grid() {
    use crate::pty::screen_state::ScreenState;
    // Drive the SAME vt100 renderer the daemon uses; the sidecar is its output.
    let bytes = b"\x1b[2J\x1b[Hhello\r\nworld\r\n";
    let mut ss = ScreenState::new();
    ss.process("pty-test", bytes);
    let rendered = ss.get_screen_text("pty-test").unwrap();

    let dir = tempfile::tempdir().unwrap();
    let screen_path = dir.path().join("screen.txt");
    std::fs::write(&screen_path, &rendered).unwrap();

    let back = std::fs::read_to_string(&screen_path).unwrap();
    assert_eq!(back, rendered);
    assert!(back.contains("hello") && back.contains("world"));
}

#[test]
fn registry_is_zero_cost_when_idle() {
    // No capture armed → tap returns immediately and never panics.
    let reg = CaptureRegistry::global();
    assert!(!reg.is_capturing());
    reg.tap_output("nonexistent", b"data");
    reg.tap_resize("nonexistent", 80, 24);
}

#[test]
fn registry_start_stop_round_trips_through_writer_thread() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("reg.cast");
    let reg = CaptureRegistry::global();
    let pty = "reg-pty-1";
    reg.start(pty, path.clone(), 80, 24, 42).unwrap();
    reg.tap_output(pty, b"\x1b[32mok\x1b[0m");
    let out = reg.stop(pty).unwrap();
    assert_eq!(out, path);

    let (header, events) = read_cast(&path);
    assert_eq!(header["timestamp"], 42u64);
    let recovered: Vec<u8> = events
        .iter()
        .filter(|(_, code, _)| code == "o")
        .flat_map(|(_, _, data)| data.clone())
        .collect();
    assert_eq!(recovered, b"\x1b[32mok\x1b[0m");
}
