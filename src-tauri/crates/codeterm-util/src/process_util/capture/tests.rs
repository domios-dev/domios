use super::*;

#[test]
fn capture_retains_exit_and_stderr_instead_of_empty_success() {
    let error = capture_result(
        Some(1),
        false,
        b"",
        b"group container: Operation not permitted",
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "exit Some(1): group container: Operation not permitted"
    );
    assert!(capture_result(None, false, b"partial", b"terminated").is_err());
    assert_eq!(capture_result(Some(0), true, b"{}", b"").unwrap(), "{}");
}

#[test]
fn capture_bounds_output() {
    assert!(capture_result(Some(0), true, &vec![0; 4 * 1024 * 1024 + 1], b"").is_err());
    assert!(capture_result(Some(0), true, b"", &vec![0; 8193]).is_err());
}
