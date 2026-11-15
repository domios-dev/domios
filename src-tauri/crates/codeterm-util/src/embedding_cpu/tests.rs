use super::*;
use std::ffi::OsStr;
use std::process::Command;

#[test]
fn daemon_reexec_strips_only_an_inherited_injected_default() {
    for (injected, inherited, removed) in [
        (true, Some("2"), true),
        (false, Some("2"), false),
        (false, Some("8"), false),
        (true, Some("8"), false),
        (true, None, false),
    ] {
        let mut command = Command::new("domios-server");
        strip_inherited_default(&mut command, injected, inherited.map(OsStr::new));
        assert_eq!(
            command
                .get_envs()
                .any(|(key, value)| key == "CANDLE_NUM_THREADS" && value.is_none()),
            removed
        );
    }
}

#[test]
fn daemon_reexec_retains_explicit_child_environment_choices() {
    for value in [Some("2"), Some("8"), None] {
        let mut command = Command::new("domios-server");
        match value {
            Some(value) => {
                command.env("CANDLE_NUM_THREADS", value);
            }
            None => {
                command.env_remove("CANDLE_NUM_THREADS");
            }
        }
        strip_inherited_default(&mut command, true, Some(OsStr::new("2")));
        assert_eq!(
            command
                .get_envs()
                .find(|(key, _)| *key == "CANDLE_NUM_THREADS")
                .unwrap()
                .1,
            value.map(OsStr::new)
        );
    }
}
