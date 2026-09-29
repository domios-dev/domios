use super::*;

#[test]
fn parse_prompt_native_js_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (n, j) = pair();
    let text = "I'll run this command:\n\n  Bash(npm test)\n\nDo you want to proceed?\n  ❯ 1. Yes\n    2. Yes, and don't ask again for Bash commands\n    3. No\n\nEsc to cancel · Tab to amend";
    let native = n.parse_prompt(text);
    let js = j.parse_prompt(text);
    let native = native.expect("native must parse the permission menu");
    let js = js.expect("plugin must parse the permission menu");
    assert_eq!(native.question, js.question, "prompt question must match");
    let native_labels: Vec<&str> = native.options.iter().map(|o| o.label.as_str()).collect();
    let js_labels: Vec<&str> = js.options.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(native_labels, js_labels, "prompt option labels must match");
}

#[test]
fn parse_prompt_unknown_footer_native_js_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (n, j) = pair();
    let text = "I'll run this command:\n\n  Bash(npm test)\n\nProceed?\n  ❯ 1. Yes\n    2. No\n\n↑/↓ navigate · Enter accept";
    let native = n.parse_prompt(text).expect("native must parse menu");
    let js = j.parse_prompt(text).expect("plugin must parse menu");
    let native_labels: Vec<&str> = native.options.iter().map(|o| o.label.as_str()).collect();
    let js_labels: Vec<&str> = js.options.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(
        native_labels, js_labels,
        "unknown footer must not enter labels"
    );
    assert_eq!(js_labels, vec!["Yes", "No"]);
}

#[test]
fn parse_prompt_negative_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (n, j) = pair();
    let text = "Reading src/router.ts and tracing the middleware chain.\n(esc to interrupt)\n";
    assert!(n.parse_prompt(text).is_none(), "native must see no prompt");
    assert!(j.parse_prompt(text).is_none(), "plugin must see no prompt");
}

#[test]
fn stop_sequence_native_js_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (n, j) = pair();
    assert_eq!(
        n.stop_keystroke(),
        j.stop_keystroke(),
        "native and plugin stop_keystroke must agree"
    );
    assert_eq!(
        n.stop_keystroke(),
        Some("\u{1b}"),
        "Claude interrupts on Esc"
    );
    assert_eq!(n.stop_keys(), j.stop_keys());
    assert_eq!(n.stop_key_wait_for_output(), j.stop_key_wait_for_output());
}

#[test]
fn model_launch_args_native_js_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (n, j) = pair();
    assert_eq!(n.model_launch_args(""), j.model_launch_args(""));
    assert!(
        n.model_launch_args("").is_empty(),
        "empty model produces no launch flags"
    );
    let native = n.model_launch_args("opus");
    let js = j.model_launch_args("opus");
    assert_eq!(native, js, "native and plugin model_launch_args must agree");
    assert_eq!(native, vec!["--model".to_string(), "opus".to_string()]);
}

#[test]
fn in_tab_command_surface_native_js_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (n, j) = pair();
    assert_eq!(
        n.compact_command(),
        j.compact_command(),
        "compact_command must agree"
    );
    assert_eq!(
        n.exit_command(),
        j.exit_command(),
        "exit_command must agree"
    );
    assert_eq!(
        n.fresh_context_command(),
        j.fresh_context_command(),
        "fresh_context_command must agree"
    );
    assert_eq!(
        n.model_switch(),
        j.model_switch(),
        "model_switch must agree"
    );
    assert_eq!(n.compact_command(), Some("/compact"));
    assert_eq!(n.fresh_context_command(), Some("/new"));
    assert_eq!(n.exit_command(), Some("/exit"));
    assert_eq!(
        n.model_switch(),
        crate::provider::ModelSwitchMode::RestartResume
    );
}

#[test]
fn needs_structured_prompt_server_native_js_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (n, j) = pair();
    assert_eq!(
        n.needs_structured_prompt_server(),
        j.needs_structured_prompt_server(),
        "native and plugin needs_structured_prompt_server must agree"
    );
    assert!(
        !n.needs_structured_prompt_server(),
        "Claude is a pull-handle provider — no host-bound server"
    );
}
