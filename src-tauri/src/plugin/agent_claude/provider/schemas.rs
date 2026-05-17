//! Settings + notification schemas, model catalog, and bridge-rule install.

use crate::agent::provider::{
    self, ActionStyle, BadgeStyle, BadgeVariant, ModelInfo, PathOpenKind, SettingField,
};

pub(super) fn builtin_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "opus".into(),
            display_name: "Claude Opus 4.7 (1M context)".into(),
            description: Some("Most capable for complex work.".into()),
        },
        ModelInfo {
            id: "sonnet".into(),
            display_name: "Claude Sonnet 4.6".into(),
            description: Some("Best for everyday tasks. Default for most work.".into()),
        },
        ModelInfo {
            id: "haiku".into(),
            display_name: "Claude Haiku 4.5".into(),
            description: Some("Fastest for quick answers.".into()),
        },
    ]
}

/// Resolved list — registered defaults unless the user dropped
/// `<data_dir>/models/claude.toml` to override.
pub(super) fn available_models() -> Vec<ModelInfo> {
    crate::agent::model_catalog::resolve_models("claude")
}

pub(super) fn notifications_schema() -> Vec<SettingField> {
    // Status badge from check_provider_integration + Install action that
    // calls install_provider_integration. Both target the generic
    // dispatcher with `provider_id: "claude"` as the arg payload — the
    // provider trait decides what "integration" means (hooks file for
    // Claude, no-op for Codex/Opencode).
    let claude_args = Some(serde_json::json!({ "provider_id": "claude" }));
    vec![SettingField::Section {
        title: "Claude Code Notifications".into(),
        description: Some(
            "Get notified when Claude Code needs permission or finishes a task. \
             Installs PostToolUse / Stop hooks into ~/.claude/settings.json that POST \
             back to CodeTerm so notifications fire on the right pane."
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
                args: claude_args,
                confirm: None,
                style: ActionStyle::Primary,
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
            crate::agent::model_catalog::resolve_default("claude").as_deref(),
        ),
        // ── Section: Rule (Claude-specific instruction file) ──
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
        std::fs::remove_file(&path)
            .map_err(|e| format!("remove {}: {}", path.display(), e))?;
        log::info!("[agent-claude] bridge rule removed from {}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::provider::AgentProvider;

    #[test]
    fn claude_metadata_describes_three_models() {
        let p = super::super::ClaudeProvider::new();
        let models = p.available_models();
        assert_eq!(models.len(), 3);
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&"sonnet"));
        assert!(ids.contains(&"opus"));
        assert!(ids.contains(&"haiku"));
        assert_eq!(p.default_model().as_deref(), Some("sonnet"));
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
                for f in fields {
                    if let SettingField::Badge { query_command, query_args, .. } = f {
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
                    if let SettingField::Action { command, state_query, args, .. } = f {
                        if command == "install_provider_integration"
                            && state_query.as_deref() == Some("check_provider_integration")
                            && args
                                .as_ref()
                                .and_then(|v| v.get("provider_id"))
                                .and_then(|v| v.as_str())
                                == Some("claude")
                        {
                            saw_action = true;
                        }
                    }
                }
                assert!(saw_badge, "expected Badge wired to check_provider_integration");
                assert!(saw_action, "expected Action wired to install_provider_integration");
            }
            _ => panic!("expected Section as top-level notifications field"),
        }
    }
}
