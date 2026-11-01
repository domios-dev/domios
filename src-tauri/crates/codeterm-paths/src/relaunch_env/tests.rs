use super::*;

fn clean(env: &[(&str, &str)]) -> Vec<(String, String)> {
    clean_relaunch_env_from(
        env.iter()
            .map(|(key, value)| (key.into(), value.into()))
            .collect(),
    )
    .into_iter()
    .map(|(key, value)| (key.into_string().unwrap(), value.into_string().unwrap()))
    .collect()
}

#[test]
fn agent_tab_relaunch_restores_custom_user_configuration() {
    assert_eq!(
        clean(&[
            ("HOME", "/Users/person"),
            ("PATH", "/usr/bin"),
            ("ZDOTDIR", "/Users/person/.domios/shell-integration"),
            ("CODETERM_ORIG_ZDOTDIR", "/Users/person/config/zsh"),
            (
                "CODETERM_INTEGRATION_ZDOTDIR",
                "/Users/person/.domios/shell-integration"
            ),
            (
                "CODETERM_USER_ZDOTDIR",
                "/Users/person/.domios/shell-integration"
            ),
            ("CODETERM_API_TOKEN", "opaque"),
            ("codeterm_tab_id", "tab"),
            ("CODETERM", "1"),
            ("BASH_ENV", "/Users/person/.domios/shell-integration/bashrc"),
            ("TERM", "xterm-256color"),
            ("COLORTERM", "truecolor"),
            ("CLICOLOR", "1"),
            ("TERM_PROGRAM", "CodeTerm"),
            ("TERM_PROGRAM_VERSION", "1.15.1"),
            (
                "XDG_DATA_DIRS",
                if cfg!(windows) {
                    "/Users/person/.domios/shell-integration;/usr/share"
                } else {
                    "/Users/person/.domios/shell-integration:/usr/share"
                }
            ),
        ]),
        vec![
            ("HOME".into(), "/Users/person".into()),
            ("PATH".into(), "/usr/bin".into()),
            ("XDG_DATA_DIRS".into(), "/usr/share".into()),
            ("ZDOTDIR".into(), "/Users/person/config/zsh".into()),
        ]
    );
}

#[test]
fn dock_and_custom_user_environment_survive() {
    let input = [
        ("HOME", "/Users/person"),
        ("ZDOTDIR", "/config/shell-integration"),
        ("BASH_ENV", "/config/bash-env"),
        ("ENV", "/config/sh-env"),
        ("TERM_PROGRAM", "OtherTerminal"),
        ("CODETERMINAL", "user"),
    ];
    assert_eq!(
        clean(&input),
        input
            .map(|(key, value)| (key.into(), value.into()))
            .to_vec()
    );
}

#[test]
fn inherited_bad_original_is_never_restored() {
    assert_eq!(
        clean(&[
            ("ZDOTDIR", "/home/user/.domios-dev-test/shell-integration/"),
            (
                "CODETERM_ORIG_ZDOTDIR",
                "/home/user/.domios/shell-integration"
            ),
            (
                "CODETERM_USER_ZDOTDIR",
                "/home/user/.domios/shell-integration"
            ),
        ]),
        vec![]
    );
}

#[test]
fn explicit_integration_location_is_scrubbed() {
    assert_eq!(
        filter_relaunch_env([
            ("CODETERM_INTEGRATION_ZDOTDIR", "/custom/injected"),
            ("ZDOTDIR", "/custom/injected/"),
            ("BASH_ENV", "/custom/injected/bashrc"),
            ("ENV", "/custom/injected-other"),
            ("κλειδί", "untouched"),
        ]),
        vec![("ENV", "/custom/injected-other"), ("κλειδί", "untouched")]
    );
}

#[test]
fn user_directory_model_rejects_self_and_preserves_custom() {
    let home = Path::new("/home/user");
    let integration = Path::new("/home/user/.domios/shell-integration");
    assert_eq!(user_zdotdir(integration, integration, home), home);
    assert_eq!(user_zdotdir(Path::new(""), integration, home), home);
    assert_eq!(
        user_zdotdir(Path::new("/custom/zsh"), integration, home),
        Path::new("/custom/zsh")
    );
}

#[test]
fn custom_integration_marker_and_empty_original_cannot_be_restored() {
    assert_eq!(
        clean(&[
            ("CODETERM_INTEGRATION_ZDOTDIR", "/custom/injected"),
            ("CODETERM_ORIG_ZDOTDIR", "/custom/injected"),
            ("ZDOTDIR", "/custom/injected"),
        ]),
        vec![]
    );
    assert_eq!(clean(&[("CODETERM_ORIG_ZDOTDIR", "")]), vec![]);
}

#[test]
fn application_restart_keeps_explicit_instance_but_drops_agent_credentials() {
    let env = vec![
        ("CODETERM_DATA_DIR".into(), "/isolated/data".into()),
        ("CODETERM_CONFIG_DIR".into(), "/isolated/config".into()),
        ("CODETERM_INSTANCE".into(), "isolated".into()),
        ("CODETERM_DEV_INSTANCE".into(), "isolated".into()),
        ("CODETERM_DEV_API_PORT".into(), "17686".into()),
        ("CODETERM_API_TOKEN".into(), "credential".into()),
        ("CODETERM_TAB_ID".into(), "old-tab".into()),
    ];
    let clean = application_relaunch_env_from(env.clone());
    assert_eq!(clean, env[..5]);
    assert!(clean_relaunch_env_from(env).is_empty());
}

#[cfg(unix)]
#[test]
fn non_utf8_environment_keys_and_values_survive_cleaning() {
    use std::os::unix::ffi::OsStringExt;
    let env = vec![(
        OsString::from_vec(vec![0xff, b'K']),
        OsString::from_vec(vec![0xfe]),
    )];
    assert_eq!(clean_relaunch_env_from(env.clone()), env);
}

#[test]
fn application_development_restart_preserves_runtime_identity_only_in_debug() {
    let env = vec![
        ("CODETERM_DEV_INSTANCE".into(), "harbor".into()),
        ("CODETERM_DEV_API_PORT".into(), "17686".into()),
        ("CODETERM_VITE_PORT".into(), "1821".into()),
        (
            "CODETERM_DEV_IDENTIFIER".into(),
            "com.domios.dev.harbor".into(),
        ),
        ("CODETERM_CDP_PORT".into(), "9321".into()),
        ("CODETERM_API_TOKEN".into(), "old-token".into()),
        ("CODETERM_TAB_ID".into(), "old-tab".into()),
    ];
    assert_eq!(application_relaunch_env_for(env.clone(), false), env[..2]);
    let captured = ApplicationRelaunchEnv(application_relaunch_env_for(env.clone(), true));
    let mut command = std::process::Command::new("domios");
    captured.apply(&mut command);
    for (key, value) in &env[..5] {
        assert!(command
            .get_envs()
            .any(|(actual, set)| actual == key && set == Some(value.as_os_str())));
    }
    assert_eq!(command.get_envs().count(), 5);
}

#[test]
fn application_production_restart_keeps_previous_filtering() {
    let env = vec![
        ("CODETERM_VITE_PORT".into(), "1821".into()),
        (
            "CODETERM_DEV_IDENTIFIER".into(),
            "com.domios.dev.harbor".into(),
        ),
        ("CODETERM_CDP_PORT".into(), "9321".into()),
        ("CODETERM_API_PORT".into(), "7685".into()),
        ("CODETERM_API_TOKEN".into(), "old-token".into()),
        ("PATH".into(), "opaque-path".into()),
    ];
    let expected = vec![("PATH".into(), "opaque-path".into())];
    assert_eq!(application_relaunch_env_for(env.clone(), false), expected);
    assert_eq!(application_relaunch_env_for(env, true), expected);
    let invalid = vec![
        ("CODETERM_DEV_API_PORT".into(), "7685".into()),
        (
            "CODETERM_DEV_IDENTIFIER".into(),
            "com.domios.dev.harbor".into(),
        ),
    ];
    assert_eq!(
        application_relaunch_env_for(invalid.clone(), true),
        invalid[..1]
    );
}

#[test]
fn captured_application_environment_outlives_startup_scrubbing() {
    let captured = ApplicationRelaunchEnv(application_relaunch_env_from(vec![
        ("CODETERM_DEV_INSTANCE".into(), "harbor".into()),
        ("CODETERM_DEV_API_PORT".into(), "17686".into()),
        ("CODETERM_API_TOKEN".into(), "old-token".into()),
    ]));
    let mut command = std::process::Command::new("domios");
    captured.apply(&mut command);
    let env: Vec<_> = command
        .get_envs()
        .map(|(key, value)| (key.to_owned(), value.unwrap().to_owned()))
        .collect();
    assert_eq!(
        env,
        vec![
            ("CODETERM_DEV_API_PORT".into(), "17686".into()),
            ("CODETERM_DEV_INSTANCE".into(), "harbor".into()),
        ]
    );
}
