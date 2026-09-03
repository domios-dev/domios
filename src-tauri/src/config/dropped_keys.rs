/// `section.key` pairs removed from the schema with no successor. Each pair
/// is the last TOML location the key lived at before removal:
/// - `orchestration.nudge_after_turns` / `orchestration.idle_poke`: retired
///   with the plan-reminder rework — `nudge_interval_secs` is time-based, not
///   a rename target for either.
/// - `keybindings.close_pane` / `keybindings.focus_pane_*`: split-pane
///   navigation itself was removed; there is no successor keybinding.
const RETIRED_KEYS: &[(&str, &str)] = &[
    ("orchestration", "nudge_after_turns"),
    ("orchestration", "idle_poke"),
    ("keybindings", "close_pane"),
    ("keybindings", "focus_pane_left"),
    ("keybindings", "focus_pane_right"),
    ("keybindings", "focus_pane_up"),
    ("keybindings", "focus_pane_down"),
];

/// Scans the raw (pre-parse) TOML for keys the current schema no longer
/// defines. `AppConfig` has no catch-all field for these sections, so
/// `toml::from_str::<AppConfig>` silently drops them — this is the only point
/// that still sees them, which is why detection reads `contents` directly
/// instead of the parsed config. Returns `section.key` paths in declaration
/// order for the caller to name in a user-facing notice.
pub(super) fn detect(contents: &str) -> Vec<String> {
    let Ok(raw) = toml::from_str::<toml::Value>(contents) else {
        return Vec::new();
    };
    RETIRED_KEYS
        .iter()
        .filter(|(section, key)| {
            raw.get(section)
                .and_then(toml::Value::as_table)
                .is_some_and(|table| table.contains_key(*key))
        })
        .map(|(section, key)| format!("{section}.{key}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_retired_orchestration_and_keybinding_keys() {
        let contents = r#"
[orchestration]
nudge_after_turns = 5

[orchestration.idle_poke]
enabled = true

[keybindings]
close_pane = "Ctrl+Shift+Q"
focus_pane_left = "Alt+ArrowLeft"
"#;
        assert_eq!(
            detect(contents),
            vec![
                "orchestration.nudge_after_turns".to_string(),
                "orchestration.idle_poke".to_string(),
                "keybindings.close_pane".to_string(),
                "keybindings.focus_pane_left".to_string(),
            ]
        );
    }

    #[test]
    fn a_config_without_retired_keys_detects_nothing() {
        let contents = "[orchestration]\nnudge_interval_secs = 1800\n\n[keybindings]\nnew_tab = \"Ctrl+T\"\n";
        assert!(detect(contents).is_empty());
    }

    #[test]
    fn an_unparseable_file_detects_nothing_rather_than_panicking() {
        assert!(detect("not = [valid toml").is_empty());
    }
}
