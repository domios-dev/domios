use super::*;

#[derive(Default)]
struct FakeDestination {
    payload: Option<String>,
    status: &'static str,
}

impl FakeDestination {
    fn accept(&mut self, payload: String, status: &'static str) {
        self.payload = Some(payload);
        self.status = status;
    }
}

#[test]
fn nested_utf8_is_preserved_and_traversal_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let pad = temp.path().join("full-tab-id");
    fs::create_dir(&pad).unwrap();
    fs::create_dir(pad.join("nested")).unwrap();
    fs::write(pad.join("nested/report.md"), "quotes ` $ Привіт\n\n").unwrap();
    assert_eq!(
        read_from_pad_root(&pad, "nested/report.md").unwrap(),
        "quotes ` $ Привіт\n\n"
    );
    assert!(read_from_pad_root(&pad, "../other.md").is_err());
    assert!(read_from_pad_root(&pad, "nested").is_err());
    assert_eq!(
        read_from_pad_root(&pad, "nested/report.md").unwrap(),
        "quotes ` $ Привіт\n\n"
    );
}

#[test]
fn current_pad_never_falls_back_without_current_tab_identity() {
    let old = std::env::var_os("CODETERM_TAB_ID");
    std::env::remove_var("CODETERM_TAB_ID");
    assert!(current_pad()
        .unwrap_err()
        .contains("focused-tab fallback is not allowed"));
    if let Some(value) = old {
        std::env::set_var("CODETERM_TAB_ID", value);
    }
}

#[test]
fn pad_enumeration_keeps_the_canonical_root_display_normalization_would_break() {
    let temp = tempfile::tempdir().unwrap();
    let pad = temp.path().join("containment-full-tab-id");
    fs::create_dir(&pad).unwrap();
    fs::create_dir(pad.join("nested")).unwrap();
    fs::write(pad.join("nested/report.md"), "body").unwrap();
    let canonical = pad.canonicalize().unwrap();

    let mut files = Vec::new();
    collect(&canonical, &canonical, &mut files).unwrap();
    assert_eq!(files, vec![PathBuf::from("nested").join("report.md")]);

    let displayed = PathBuf::from(display_pad_path(&canonical));
    let mut from_displayed_root = Vec::new();
    let result = collect(&displayed, &canonical, &mut from_displayed_root);
    if cfg!(windows) {
        assert_eq!(
            result.unwrap_err(),
            "AgentPad entry escapes the current pad",
            "the display form is not a containment root on Windows"
        );
    } else {
        assert_eq!(displayed, canonical);
        result.unwrap();
    }
}

#[cfg(unix)]
#[test]
fn symlink_escape_is_rejected_before_read() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let pad = temp.path().join("full-tab-symlink");
    fs::create_dir(&pad).unwrap();
    let outside = temp.path().join("outside.md");
    fs::write(&outside, "outside").unwrap();
    symlink(outside, pad.join("escape.md")).unwrap();
    assert!(read_from_pad_root(&pad, "escape.md").is_err());
    assert!(read_from_pad_root(&pad, pad.join("escape.md").to_str().unwrap()).is_err());
}

#[test]
fn send_from_pad_resolves_before_transport() {
    let temp = tempfile::tempdir().unwrap();
    let pad = temp.path().join("send-owner-full-id");
    fs::create_dir(&pad).unwrap();
    let expected = "reusable `payload` $value Привіт\nsecond line\n";
    fs::write(pad.join("message.md"), expected).unwrap();
    let first = read_from_pad_root(&pad, "message.md").unwrap();
    let second = read_from_pad_root(&pad, "message.md").unwrap();
    let mut transport = FakeDestination::default();
    transport.accept(first, "accepted");
    assert_eq!(transport.payload.as_deref(), Some(expected));
    assert_eq!(second, expected);
    assert!(!transport
        .payload
        .as_ref()
        .unwrap()
        .contains(pad.to_string_lossy().as_ref()));
}

#[test]
fn message_file_dash_reads_stdin() {
    for option in ["--message-file", "--from-file", "--from-pad", "--stdin"] {
        let expected = "quoted `body` $value\r\nλ😀\n";
        let mut stdin = std::io::Cursor::new(expected);
        assert_eq!(
            read_file_or_stdin_with("-", option, &mut stdin)
                .unwrap()
                .as_bytes(),
            expected.as_bytes()
        );
    }
}

#[test]
fn agent_pad_report_reaches_parent_and_survives_worker_close() {
    let temp = tempfile::tempdir().unwrap();
    let pad = temp.path().join("worker-full-id");
    fs::create_dir(&pad).unwrap();
    let expected =
        "STATUS: in_review\n\n- quotes: '\"'\n- code: `x`\n- dollar: $HOME\n- Unicode: Привіт\n";
    fs::write(pad.join("report.md"), expected).unwrap();
    let resolved = read_from_pad_root(&pad, "report.md").unwrap();
    let mut parent = FakeDestination::default();
    parent.accept(resolved, "in_review");
    fs::remove_dir_all(&pad).unwrap();
    assert_eq!(parent.payload.as_deref(), Some(expected));
    assert_eq!(parent.status, "in_review");
    assert!(!pad.exists());
}

#[test]
fn absolute_sources_are_confined_to_the_callers_pad() {
    let temp = tempfile::tempdir().unwrap();
    let pad = temp.path().join("caller");
    fs::create_dir(&pad).unwrap();
    let own = pad.join("note.md");
    fs::write(&own, "round note").unwrap();
    let outside = temp.path().join("note.md");
    fs::write(&outside, "foreign").unwrap();
    assert_eq!(
        read_from_pad_root(&pad, own.to_str().unwrap()).unwrap(),
        "round note"
    );
    assert!(read_from_pad_root(&pad, outside.to_str().unwrap()).is_err());
    assert!(read_from_pad_root(&pad, pad.to_str().unwrap()).is_err());
    assert!(read_from_pad_root(&pad, pad.join("../note.md").to_str().unwrap()).is_err());
}
