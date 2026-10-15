use super::*;

#[test]
fn launch_failure_screen_tail_keeps_error_before_returned_shell_prompt() {
    let error = "Error: Invalid JSON provided to --settings";
    let screen = format!("old row\nolder row\nlaunch command\n\n{error}\nPS D:\\work>\n\n");
    assert_eq!(
        screen_tail(&screen),
        vec!["older row", "launch command", error, "PS D:\\work>"]
    );
    assert!(screen_tail(" \n\t\r\n").is_empty());
}

#[test]
fn launch_failure_screen_tail_bounds_unicode_and_removes_control_characters() {
    let screen = format!("\0{}\u{7}", "界".repeat(MAX_LINE_CHARS + 1));
    let tail = screen_tail(&screen);
    assert_eq!(tail, vec!["界".repeat(MAX_LINE_CHARS)]);
}
