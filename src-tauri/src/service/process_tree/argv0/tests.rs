use super::*;

#[test]
fn cmdline_extracts_only_the_argv0_basename() {
    assert_eq!(
        from_cmdline(b"/opt/bin/agent-cli\0--model\0other\0"),
        Some("agent-cli".into())
    );
    assert_eq!(from_cmdline(b"agent-cli\0"), Some("agent-cli".into()));
    for bytes in [
        b"".as_slice(),
        b"\0agent-cli\0",
        b"unterminated",
        b"/\0",
        b"\xff\0",
    ] {
        assert_eq!(from_cmdline(bytes), None);
    }
}

#[test]
fn procargs_skips_the_executable_path_and_padding() {
    let mut bytes = 2_i32.to_ne_bytes().to_vec();
    bytes
        .extend_from_slice(b"/opt/bin/native-helper\0\0\0/opt/bin/agent-cli\0--model\0KEY=value\0");
    assert_eq!(from_procargs(&bytes), Some("agent-cli".into()));
}

#[test]
fn procargs_rejects_missing_or_invalid_argv0() {
    for suffix in [
        b"".as_slice(),
        b"/opt/helper",
        b"/opt/helper\0\0",
        b"/opt/helper\0unterminated",
        b"/opt/helper\0\xff\0",
    ] {
        let mut bytes = 1_i32.to_ne_bytes().to_vec();
        bytes.extend_from_slice(suffix);
        assert_eq!(from_procargs(&bytes), None);
    }
    assert_eq!(from_procargs(&[1, 0, 0]), None);
    let mut bytes = 0_i32.to_ne_bytes().to_vec();
    bytes.extend_from_slice(b"/opt/helper\0agent-cli\0");
    assert_eq!(from_procargs(&bytes), None);
}
