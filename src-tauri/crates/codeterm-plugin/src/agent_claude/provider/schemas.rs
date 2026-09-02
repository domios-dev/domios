//! Settings + notification schemas, model catalog, and bridge-rule install.

use crate::provider::{
    self, ActionStyle, BadgeStyle, BadgeVariant, ModelInfo, PathOpenKind, ReasoningEffortInfo,
    SettingField,
};

/// Bundled manifest is the single shipped Claude alias catalog; the native
/// provider registers this exact seed, never a mirrored Rust vector.
const BUNDLED_MANIFEST: &str =
    include_str!("../../../../../../plugins/provider-claude/plugin.json");

pub(crate) fn manifest_defaults() -> crate::model_catalog::RegisteredDefaults {
    static SEED: std::sync::OnceLock<crate::model_catalog::RegisteredDefaults> =
        std::sync::OnceLock::new();
    SEED.get_or_init(|| {
        let manifest: crate::js_runtime::PluginManifest =
            serde_json::from_str(BUNDLED_MANIFEST).expect("bundled Claude manifest parses");
        crate::model_catalog::RegisteredDefaults {
            models: manifest.models.iter().map(ModelInfo::from).collect(),
            default: manifest.default_model,
        }
    })
    .clone()
}

pub(crate) fn reasoning_effort_defaults() -> (Vec<ReasoningEffortInfo>, Option<String>) {
    let manifest: crate::js_runtime::PluginManifest =
        serde_json::from_str(BUNDLED_MANIFEST).expect("bundled Claude manifest parses");
    (
        manifest
            .reasoning_efforts
            .iter()
            .map(ReasoningEffortInfo::from)
            .collect(),
        manifest.default_reasoning_effort,
    )
}

pub(crate) fn reasoning_effort_launch_args_format() -> Vec<String> {
    let manifest: crate::js_runtime::PluginManifest =
        serde_json::from_str(BUNDLED_MANIFEST).expect("bundled Claude manifest parses");
    manifest.commands.reasoning_effort_launch_args_format
}

/// Resolved list — registered defaults unless the user dropped
/// `<data_dir>/models/claude.toml` to override.
pub(super) fn available_models() -> Vec<ModelInfo> {
    crate::model_catalog::resolve_models("claude")
}

pub(super) fn notifications_schema() -> Vec<SettingField> {
    // with `provider_id: "claude"` as the arg payload — the provider trait decides what "integration" means (hooks file for Claude, no-op for Codex/Opencode).
    let claude_args = Some(serde_json::json!({ "provider_id": "claude" }));
    vec![SettingField::Section {
        title: "Claude Code Notifications".into(),
        description: Some(
            "Get notified when Claude Code needs permission or finishes a task. \
             Installs PostToolUse / Stop hooks into ~/.claude/settings.json that POST \
             back to CodeTerm so notifications fire on the right tab."
                .into(),
        ),
        fields: vec![
            SettingField::PathLink {
                label: "Settings file".into(),
                path: "~/.claude/settings.json".into(),
                open: PathOpenKind::OpenFolder,
            },
            SettingField::Badge {
                query_command: "check_provider_integration".into(),
                query_args: claude_args.clone(),
                variants: vec![
                    BadgeVariant {
                        id: "installed".into(),
                        label: "installed".into(),
                        style: BadgeStyle::Green,
                    },
                    BadgeVariant {
                        id: "missing".into(),
                        label: "not installed".into(),
                        style: BadgeStyle::Gray,
                    },
                ],
            },
            SettingField::Action {
                id: "install_hooks".into(),
                label: "Install hooks".into(),
                command: "install_provider_integration".into(),
                state_query: Some("check_provider_integration".into()),
                args: Some(serde_json::json!({
                    "provider_id": "claude",
                    "consent": true,
                    "precompact_memory_export": false
                })),
                confirm: Some("Review target ~/.claude/settings.json. CodeTerm will back it up, preserve foreign keys/hooks, and add Notification, Stop, and SessionStart callbacks. Continue?".into()),
                style: ActionStyle::Primary,
            },
            SettingField::Action {
                id: "install_hooks_with_memory_export".into(),
                label: "Install with PreCompact memory export".into(),
                command: "install_provider_integration".into(),
                state_query: None,
                args: Some(serde_json::json!({
                    "provider_id": "claude",
                    "consent": true,
                    "precompact_memory_export": true
                })),
                confirm: Some("This separately opts in to exporting conversation context to CodeTerm memory before Claude compacts. The settings file will be backed up first. Continue?".into()),
                style: ActionStyle::Secondary,
            },
            SettingField::Action {
                id: "uninstall_hooks".into(),
                label: "Uninstall CodeTerm hooks".into(),
                command: "uninstall_provider_integration".into(),
                state_query: None,
                args: claude_args.clone(),
                confirm: Some("Remove only CodeTerm-owned hooks and preserve foreign configuration?".into()),
                style: ActionStyle::Secondary,
            },
            SettingField::Action {
                id: "restore_hooks_backup".into(),
                label: "Restore pre-install backup".into(),
                command: "restore_provider_integration".into(),
                state_query: None,
                args: claude_args,
                confirm: Some("Restore the recoverable pre-install backup over the current Claude settings file?".into()),
                style: ActionStyle::Danger,
            },
        ],
    }]
}

pub(super) fn settings_schema() -> Vec<SettingField> {
    vec![
        provider::session_resume_section(
            "Automatically resume Claude Code sessions when CodeTerm restarts.",
            "--dangerously-skip-permissions",
        ),
        provider::model_select_section(
            "Default model for new Claude Code sessions.",
            available_models(),
            crate::model_catalog::resolve_default("claude").as_deref(),
        ),
        SettingField::Section {
            title: "Rule".into(),
            description: Some(
                "Teaches Claude Code when and how to use CodeTerm — auto-rename tabs, use workspaces instead of mkdir, open browser panel instead of external browser.".into(),
            ),
            fields: vec![
                SettingField::PathLink {
                    label: "Rule file".into(),
                    path: "~/.claude/rules/codeterm.md".into(),
                    open: PathOpenKind::OpenFolder,
                },
                SettingField::Badge {
                    query_command: "check_provider_rule".into(),
                    query_args: Some(serde_json::json!({ "provider_id": "claude" })),
                    variants: vec![
                        BadgeVariant {
                            id: "installed".into(),
                            label: "installed".into(),
                            style: BadgeStyle::Green,
                        },
                        BadgeVariant {
                            id: "update".into(),
                            label: "update available".into(),
                            style: BadgeStyle::Orange,
                        },
                        BadgeVariant {
                            id: "missing".into(),
                            label: "not installed".into(),
                            style: BadgeStyle::Gray,
                        },
                    ],
                },
                SettingField::Action {
                    id: "install_rule".into(),
                    label: "Install".into(),
                    command: "install_provider_rule".into(),
                    state_query: Some("check_provider_rule".into()),
                    args: Some(serde_json::json!({ "provider_id": "claude" })),
                    confirm: None,
                    style: ActionStyle::Primary,
                },
            ],
        },
    ]
}

pub(super) fn install_bridge_rule(payload: &str) -> Result<(), String> {
    let rules_dir = dirs::home_dir()
        .ok_or_else(|| "no home directory".to_string())?
        .join(".claude")
        .join("rules");
    std::fs::create_dir_all(&rules_dir)
        .map_err(|e| format!("create {}: {}", rules_dir.display(), e))?;

    let final_path = rules_dir.join("codeterm.md");
    let tmp_path = rules_dir.join("codeterm.md.tmp");

    std::fs::write(&tmp_path, payload)
        .map_err(|e| format!("write {}: {}", tmp_path.display(), e))?;
    std::fs::rename(&tmp_path, &final_path)
        .map_err(|e| format!("rename → {}: {}", final_path.display(), e))?;

    log::info!(
        "[agent-claude] bridge rule installed to {}",
        final_path.display()
    );
    Ok(())
}

pub(super) fn uninstall_bridge_rule() -> Result<(), String> {
    let path = dirs::home_dir()
        .ok_or_else(|| "no home directory".to_string())?
        .join(".claude")
        .join("rules")
        .join("codeterm.md");
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("remove {}: {}", path.display(), e))?;
        log::info!("[agent-claude] bridge rule removed from {}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::AgentProvider;

    #[test]
    fn claude_seed_is_manifest_derived_stable_aliases_with_sonnet_default() {
        let p = super::super::ClaudeProvider::new();
        let ids: Vec<String> = p.available_models().into_iter().map(|m| m.id).collect();
        assert_eq!(ids, vec!["opus", "sonnet", "haiku", "fable"]);
        assert_eq!(p.default_model().as_deref(), Some("sonnet"));
        let seed = manifest_defaults();
        assert_eq!(seed.default.as_deref(), Some("sonnet"));
        assert!(seed
            .models
            .iter()
            .filter(|m| m.id != "fable")
            .all(|m| m.available.is_none()));
        assert_eq!(
            seed.models
                .iter()
                .find(|m| m.id == "fable")
                .and_then(|m| m.available),
            Some(false),
            "the version-gated alias must fail closed until discovery confirms the CLI"
        );
    }

    #[test]
    fn claude_settings_schema_has_required_sections() {
        let p = super::super::ClaudeProvider::new();
        let schema = p.settings_schema();
        let section_titles: Vec<&str> = schema
            .iter()
            .filter_map(|f| match f {
                SettingField::Section { title, .. } => Some(title.as_str()),
                _ => None,
            })
            .collect();
        assert!(section_titles.contains(&"Session Resume"));
        assert!(section_titles.contains(&"Model"));
        assert!(section_titles.contains(&"Rule"));
    }

    #[test]
    fn claude_notifications_schema_present() {
        let p = super::super::ClaudeProvider::new();
        let schema = p.notifications_schema();
        assert!(!schema.is_empty(), "Claude must contribute notifications");
        match &schema[0] {
            SettingField::Section { title, fields, .. } => {
                assert_eq!(title, "Claude Code Notifications");
                let mut saw_badge = false;
                let mut saw_action = false;
                let mut saw_restore = false;
                for f in fields {
                    if let SettingField::Badge {
                        query_command,
                        query_args,
                        ..
                    } = f
                    {
                        if query_command == "check_provider_integration"
                            && query_args
                                .as_ref()
                                .and_then(|v| v.get("provider_id"))
                                .and_then(|v| v.as_str())
                                == Some("claude")
                        {
                            saw_badge = true;
                        }
                    }
                    if let SettingField::Action {
                        command,
                        state_query,
                        args,
                        ..
                    } = f
                    {
                        if command == "install_provider_integration"
                            && state_query.as_deref() == Some("check_provider_integration")
                            && args
                                .as_ref()
                                .and_then(|v| v.get("provider_id"))
                                .and_then(|v| v.as_str())
                                == Some("claude")
                            && args
                                .as_ref()
                                .and_then(|v| v.get("consent"))
                                .and_then(|v| v.as_bool())
                                == Some(true)
                            && args
                                .as_ref()
                                .and_then(|v| v.get("precompact_memory_export"))
                                .and_then(|v| v.as_bool())
                                == Some(false)
                        {
                            saw_action = true;
                        }
                        if command == "restore_provider_integration" {
                            saw_restore = true;
                        }
                    }
                }
                assert!(
                    saw_badge,
                    "expected Badge wired to check_provider_integration"
                );
                assert!(
                    saw_action,
                    "expected Action wired to install_provider_integration"
                );
                assert!(saw_restore, "expected recoverable restore action");
            }
            _ => panic!("expected Section as top-level notifications field"),
        }
    }
}
