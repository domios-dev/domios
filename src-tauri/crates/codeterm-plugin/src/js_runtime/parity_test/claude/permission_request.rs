//! Native/JS install parity for the permission route.
//!
//! The roster drives install on the NATIVE path only; the JS drop-in — the copy
//! shipped in the channel, and the one that displaces native at runtime — has
//! its own installer. A native-only declaration would read as true while the
//! hook file that actually gets written never gains the event, and every native
//! test would still pass. This asserts both runtimes install the same declared
//! permission events, with the payload forwarded on both.

use std::collections::BTreeSet;

use super::*;
use crate::provider::{HookIngestionRoute, IntegrationInstallOptions};

fn consented() -> IntegrationInstallOptions {
    IntegrationInstallOptions {
        consent: true,
        precompact_memory_export: false,
    }
}

fn declared_permission_events() -> BTreeSet<String> {
    ClaudeProvider::new()
        .hook_event_roster()
        .into_iter()
        .filter(|binding| binding.route == HookIngestionRoute::Permission)
        .map(|binding| binding.event)
        .collect()
}

fn installed_events(settings: &std::path::Path, declared: &BTreeSet<String>) -> BTreeSet<String> {
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(settings).unwrap()).unwrap();
    declared
        .iter()
        .filter(|event| json.pointer(&format!("/hooks/{event}/0")).is_some())
        .cloned()
        .collect()
}

fn installed_command(settings: &std::path::Path, event: &str) -> String {
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(settings).unwrap()).unwrap();
    json.pointer(&format!("/hooks/{event}/0/hooks/0/command"))
        .and_then(|command| command.as_str())
        .unwrap_or_else(|| panic!("{event} installed at {}", settings.display()))
        .to_string()
}

#[test]
fn permission_request_native_and_js_install_the_same_declared_events() {
    if require_plugins().is_none() {
        return;
    }
    let declared = declared_permission_events();
    assert!(
        declared.contains("PermissionRequest"),
        "PermissionRequest must be declared on the permission route: {declared:?}"
    );

    let native_dir = tempfile::tempdir().unwrap();
    let native_settings = native_dir.path().join("settings.json");
    crate::agent_claude::provider::integration::install_integration_at(
        &native_settings,
        &consented(),
    )
    .unwrap();

    let js_dir = tempfile::tempdir().unwrap();
    let js_settings = js_dir.path().join("settings.json");
    claude_in_temp(&js_dir.path().join("rule.md"), &js_settings)
        .install_integration(7686, &consented())
        .unwrap();

    assert_eq!(
        installed_events(&native_settings, &declared),
        declared,
        "native install must cover every declared permission event"
    );
    assert_eq!(
        installed_events(&js_settings, &declared),
        declared,
        "the shipped JS installer must cover the same declared events"
    );
}

#[test]
fn permission_request_payload_is_forwarded_on_both_runtimes() {
    if require_plugins().is_none() {
        return;
    }
    let declared = declared_permission_events();

    let native_dir = tempfile::tempdir().unwrap();
    let native_settings = native_dir.path().join("settings.json");
    crate::agent_claude::provider::integration::install_integration_at(
        &native_settings,
        &consented(),
    )
    .unwrap();

    let js_dir = tempfile::tempdir().unwrap();
    let js_settings = js_dir.path().join("settings.json");
    claude_in_temp(&js_dir.path().join("rule.md"), &js_settings)
        .install_integration(7686, &consented())
        .unwrap();

    for event in &declared {
        for settings in [&native_settings, &js_settings] {
            let command = installed_command(settings, event);
            assert!(
                command.contains("--data-binary @-"),
                "{event} must forward the stdin payload: {command}"
            );
            assert!(
                !command.contains("-d '{}'"),
                "{event} must not post an empty body: {command}"
            );
            assert!(
                command.contains("CODETERM_SESSION_BINDING_NONCE"),
                "{event} must carry the binding nonce: {command}"
            );
        }
    }
}

#[test]
fn permission_request_and_notification_keep_distinct_matchers_on_both_runtimes() {
    if require_plugins().is_none() {
        return;
    }
    let native_dir = tempfile::tempdir().unwrap();
    let native_settings = native_dir.path().join("settings.json");
    crate::agent_claude::provider::integration::install_integration_at(
        &native_settings,
        &consented(),
    )
    .unwrap();

    let js_dir = tempfile::tempdir().unwrap();
    let js_settings = js_dir.path().join("settings.json");
    claude_in_temp(&js_dir.path().join("rule.md"), &js_settings)
        .install_integration(7686, &consented())
        .unwrap();

    for settings in [&native_settings, &js_settings] {
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(settings).unwrap()).unwrap();
        assert_eq!(
            json.pointer("/hooks/PermissionRequest/0/matcher")
                .and_then(|m| m.as_str()),
            Some("*"),
            "PermissionRequest fires per tool at {}",
            settings.display()
        );
        assert_eq!(
            json.pointer("/hooks/Notification/0/matcher")
                .and_then(|m| m.as_str()),
            Some("permission_prompt"),
            "Notification keeps the vendor matcher at {}",
            settings.display()
        );
    }
}
