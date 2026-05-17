//! End-to-end integration tests that load real bundled plugins from
//! the repo's `plugins/` workspace through the production loader path.
//!
//! These tests double as the validation step for "an independent
//! developer can drop a plugin into the workspace, restart codeterm,
//! and it just works" — the cursor plugin was written without touching
//! a line of Rust, and we exercise it here through the same code path
//! that lib.rs uses at startup.

use std::path::PathBuf;

use super::{load_one, scan_roots};
use crate::agent::provider::{AgentLaunchParams, AgentProvider};

fn workspace_plugins_root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/..")).join("plugins")
}

fn cursor_plugin() -> super::JsProvider {
    let manifest = workspace_plugins_root()
        .join("provider-cursor")
        .join("plugin.json");
    load_one(&manifest).expect("load cursor plugin")
}

#[test]
fn cursor_plugin_loads_via_default_loader() {
    let root = workspace_plugins_root();
    if !root.exists() {
        // Cargo test from a release tarball without the workspace —
        // nothing to validate, skip.
        return;
    }
    let (providers, _) = scan_roots(&[root]);
    assert!(
        providers.iter().any(|p| p.id() == "cursor"),
        "scan_roots must discover the cursor plugin"
    );
}

#[test]
fn cursor_metadata_reads_off_manifest() {
    if !workspace_plugins_root().exists() {
        return;
    }
    let p = cursor_plugin();
    assert_eq!(p.id(), "cursor");
    assert_eq!(p.display_name(), "Cursor Agent");
    assert_eq!(p.binary_name(), "agent");
    assert_eq!(p.cli_binary(), Some("agent"));
    assert_eq!(p.icon_hint(), Some("▶"));
    assert_eq!(p.icon_color(), Some("#7a73ff"));
    assert!(p.preserves_context_on_resume());
    assert!(!p.has_hooks());
}

#[test]
fn cursor_detects_from_title_and_output() {
    if !workspace_plugins_root().exists() {
        return;
    }
    let p = cursor_plugin();
    assert!(p.detect_from_title("Cursor Agent — main"));
    assert!(p.detect_from_title("cursor-agent v1"));
    assert!(!p.detect_from_title("bash — main"));

    assert!(p.detect_from_output("Type /plan to switch modes"));
    assert!(!p.detect_from_output("$ ls\n"));
}

#[test]
fn cursor_launch_command_inline_task() {
    if !workspace_plugins_root().exists() {
        return;
    }
    let p = cursor_plugin();
    let params = AgentLaunchParams {
        task: Some("write a test".into()),
        headless: true,
        skip_permissions: false,
        ..Default::default()
    };
    let cmd = p.build_launch_command(&params);
    assert!(cmd.starts_with("agent -p "), "got: {cmd}");
    assert!(cmd.contains("'write a test'"));
    assert!(cmd.contains("--output-format text"));
    assert!(!cmd.contains("--sandbox"));
}

#[test]
fn cursor_launch_command_with_skip_permissions_disables_sandbox() {
    if !workspace_plugins_root().exists() {
        return;
    }
    let p = cursor_plugin();
    let params = AgentLaunchParams {
        skip_permissions: true,
        ..Default::default()
    };
    let cmd = p.build_launch_command(&params);
    assert_eq!(cmd, "agent --sandbox disabled");
}

#[test]
fn cursor_launch_command_quotes_dangerous_task_text() {
    if !workspace_plugins_root().exists() {
        return;
    }
    let p = cursor_plugin();
    let params = AgentLaunchParams {
        task: Some("rm 'foo'; echo $HOME".into()),
        headless: true,
        ..Default::default()
    };
    let cmd = p.build_launch_command(&params);
    // POSIX single-quote escape: every embedded ' becomes '\''
    assert!(cmd.contains("'rm '\\''foo'\\''; echo $HOME'"), "got: {cmd}");
}

#[test]
fn cursor_resume_command_carries_session_id() {
    if !workspace_plugins_root().exists() {
        return;
    }
    let p = cursor_plugin();
    let cmd = p.build_resume_command("chat-12345", false).unwrap();
    assert_eq!(cmd, "agent --resume 'chat-12345'");

    let cmd2 = p.build_resume_command("chat-67890", true).unwrap();
    assert_eq!(cmd2, "agent --resume 'chat-67890' --sandbox disabled");
}

#[test]
fn cursor_parse_prompt_recognises_numbered_options() {
    if !workspace_plugins_root().exists() {
        return;
    }
    let p = cursor_plugin();
    let text = "Allow this command?\n  1. Yes, just this once\n  2. No, cancel";
    let (q, opts) = p.parse_prompt(text).unwrap();
    assert_eq!(q, "Allow this command?");
    assert_eq!(opts.len(), 2);
    assert_eq!(opts[0].0, "1");
    assert!(opts[0].1.contains("Yes"));
}

#[test]
fn cursor_models_surface_through_descriptor() {
    if !workspace_plugins_root().exists() {
        return;
    }
    let p = cursor_plugin();
    let desc = p.describe();
    assert_eq!(desc.id, "cursor");
    assert_eq!(desc.default_model.as_deref(), Some("gpt-5.2"));
    assert!(desc.available_models.iter().any(|m| m.id == "gpt-5.2"));
    // model_select section auto-injected from manifest data when the
    // plugin doesn't ship its own settingsSchema.
    let has_model_section = desc.settings_schema.iter().any(|f| matches!(
        f,
        crate::agent::provider::SettingField::Section { title, .. } if title == "Model"
    ));
    assert!(has_model_section);
}

#[test]
fn cursor_install_command_per_platform() {
    if !workspace_plugins_root().exists() {
        return;
    }
    let p = cursor_plugin();
    assert!(p
        .install_command("macos")
        .unwrap()
        .contains("cursor.com/install"));
    assert!(p
        .install_command("linux")
        .unwrap()
        .contains("cursor.com/install"));
    assert!(p
        .install_command("windows")
        .unwrap()
        .contains("install.ps1"));
}

#[test]
fn cursor_status_bar_rows_drawn_from_manifest_spawn_block() {
    if !workspace_plugins_root().exists() {
        return;
    }
    let p = cursor_plugin();
    assert_eq!(p.status_bar_rows(), 2);
}

#[test]
fn cursor_model_launch_args_substitute_placeholder() {
    if !workspace_plugins_root().exists() {
        return;
    }
    let p = cursor_plugin();
    assert_eq!(
        p.model_launch_args("gpt-5.2"),
        vec!["--model".to_string(), "gpt-5.2".to_string()]
    );
}

#[test]
fn cursor_model_switch_command_from_template() {
    if !workspace_plugins_root().exists() {
        return;
    }
    let p = cursor_plugin();
    assert_eq!(
        p.model_switch_command("claude-opus-4.7").as_deref(),
        Some("/model claude-opus-4.7")
    );
}
