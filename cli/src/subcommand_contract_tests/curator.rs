use clap::Parser;

#[test]
fn curator_commands_parse_control_loop_and_scoped_grants() {
    for args in [
        vec!["codeterm", "plan", "control-loop", "show", "pl-fixture"],
        vec![
            "codeterm",
            "plan",
            "control-loop",
            "set",
            "pl-fixture",
            "curator",
        ],
        vec![
            "codeterm",
            "plan",
            "control-loop",
            "set",
            "pl-fixture",
            "heartbeat",
            "--interval",
            "30m",
        ],
        vec![
            "codeterm",
            "plan",
            "curator",
            "grant",
            "pl-fixture",
            "tab-fixture",
            "--blueprint",
            "bp-fixture",
            "--plan-skill",
            "recipe",
            "--save",
            "bp-candidate",
        ],
        vec![
            "codeterm",
            "curator",
            "finish",
            "--next",
            "90m",
            "--from-pad",
            "note.md",
        ],
    ] {
        assert!(crate::Cli::try_parse_from(&args).is_ok(), "{args:?}");
    }
    assert!(crate::Cli::try_parse_from([
        "codeterm",
        "plan",
        "control-loop",
        "set",
        "pl-fixture",
        "unknown"
    ])
    .is_err());
}
