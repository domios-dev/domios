use super::*;

#[test]
fn provider_update_output_is_drained_with_a_bounded_tail() {
    let mut bytes = vec![b'x'; 65536];
    bytes.extend(b"opaque-final-output");
    let rx = capture(std::io::Cursor::new(bytes)).unwrap();
    let output = captured(Some(rx));
    assert_eq!(output.len(), 32768);
    assert!(output.ends_with("opaque-final-output"));
}

#[test]
fn provider_update_command_rejects_shell_input() {
    assert!(update_argv("updater update; injected").is_err());
    assert!(update_argv("updater $(injected)").is_err());
    assert!(update_argv("").is_err());
}
