use super::{bounded_failure, MAX_FAILURE_CHARS};

#[test]
fn a_plugin_failure_is_stored_as_one_bounded_line() {
    assert_eq!(bounded_failure(" opaque\r\ncause "), "opaque  cause");
    assert_eq!(bounded_failure("  \n"), "Balance read failed");
    assert_eq!(
        bounded_failure(&"x".repeat(1000)).chars().count(),
        MAX_FAILURE_CHARS
    );
}
