//! Provider-hook ownership and reconciliation, against the hook forms observed
//! on real machines.
//!
//! Fixtures: the Claude `current` block is the verbatim `~/.claude/settings.json`
//! hook set read from BLACKSTAR, where Claude Code's own serializer has already
//! stripped every `_source`. The `legacy v2` block is the pane-era form reported
//! on ROG in issue 476 (`${CODETERM_API_PORT:-7685}` + `/api/panes/$CODETERM_PANE_ID/`).

use codeterm_plugin::agent_claude::provider::hook_ownership::{
    is_codeterm_hook, CODETERM_HOOK_MARKER_HEADER,
};
use codeterm_plugin::agent_claude::provider::integration::install_integration_at;
use codeterm_plugin::provider::IntegrationInstallOptions;
use serde_json::Value as Json;

const CLAUDE_EVENTS: [&str; 5] = [
    "Notification",
    "PermissionRequest",
    "Stop",
    "PreCompact",
    "SessionStart",
];

const CURRENT_PERMISSION: &str = r#"if [ -n "${CODETERM_API_URL:-}" ] && [ -n "${CODETERM_API_TOKEN:-}" ] && [ -n "${CODETERM_TAB_ID:-}" ]; then curl -s -o /dev/null -X POST "$CODETERM_API_URL/api/tabs/$CODETERM_TAB_ID/hook/permission" -H 'Content-Type: application/json' -H "Authorization: Bearer $CODETERM_API_TOKEN" -H "X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}" --data-binary @- || true; fi"#;
const CURRENT_STOP: &str = r#"if [ -n "${CODETERM_API_URL:-}" ] && [ -n "${CODETERM_API_TOKEN:-}" ] && [ -n "${CODETERM_TAB_ID:-}" ]; then curl -s -o /dev/null -X POST "$CODETERM_API_URL/api/tabs/$CODETERM_TAB_ID/hook/stop" -H 'Content-Type: application/json' -H "Authorization: Bearer $CODETERM_API_TOKEN" -d '{}' || true; fi"#;
const CURRENT_PRECOMPACT: &str = r#"if [ -n "${CODETERM_API_URL:-}" ] && [ -n "${CODETERM_API_TOKEN:-}" ] && [ -n "${CODETERM_TAB_ID:-}" ]; then curl -s -o /dev/null -X POST "$CODETERM_API_URL/api/mem/flush" -H 'Content-Type: application/json' -H "Authorization: Bearer $CODETERM_API_TOKEN" -d '{"tab":"'"$CODETERM_TAB_ID"'","title":"pre-compact"}' || true; fi"#;
const CURRENT_SESSION_START: &str = r#"if [ -n "${CODETERM_API_URL:-}" ] && [ -n "${CODETERM_API_TOKEN:-}" ] && [ -n "${CODETERM_TAB_ID:-}" ]; then curl -s -o /dev/null -X POST "$CODETERM_API_URL/api/tabs/$CODETERM_TAB_ID/hook/session_start" -H 'Content-Type: application/json' -H "Authorization: Bearer $CODETERM_API_TOKEN" -H "X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}" --data-binary @- || true; fi"#;

const LEGACY_V2_SESSION_START: &str = r#"curl -s -X POST http://localhost:${CODETERM_API_PORT:-7685}/api/panes/$CODETERM_PANE_ID/hook/session_start -H 'Content-Type: application/json' -H "X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}" -d '{}' > /dev/null 2>&1 || true"#;
const LEGACY_V2_STOP: &str = r#"curl -s -X POST http://localhost:${CODETERM_API_PORT:-7685}/api/panes/$CODETERM_PANE_ID/hook/stop -H 'Content-Type: application/json' -d '{}' > /dev/null 2>&1 || true"#;

/// A neighbour's hook that happens to talk to a local HTTP service. Nothing
/// about it is CodeTerm's, so reconciliation must leave it alone.
const FOREIGN_LOCAL_HTTP: &str =
    "curl -s -X POST http://localhost:7686/api/telemetry/session -d '{\"kind\":\"start\"}'";
const FOREIGN_SCRIPT: &str = "~/bin/my-audit-hook.sh --event stop";

fn entry(matcher: &str, command: &str, source: Option<&str>) -> Json {
    let mut hook = serde_json::json!({
        "type": "command",
        "command": command,
        "timeout": 5000,
    });
    if let Some(source) = source {
        hook["_source"] = Json::from(source);
    }
    serde_json::json!({ "matcher": matcher, "hooks": [hook] })
}

/// The verbatim BLACKSTAR set: one CodeTerm entry per event, every `_source`
/// already removed by Claude Code's own settings rewrite.
fn current_unmarked_entry(event: &str) -> Json {
    match event {
        "Notification" => entry("permission_prompt", CURRENT_PERMISSION, None),
        "PermissionRequest" => entry("*", CURRENT_PERMISSION, None),
        "Stop" => entry("", CURRENT_STOP, None),
        "PreCompact" => entry("", CURRENT_PRECOMPACT, None),
        "SessionStart" => entry("", CURRENT_SESSION_START, None),
        other => panic!("unknown Claude event {other}"),
    }
}

fn foreign_entry(event: &str) -> Json {
    match event {
        "Stop" => entry("", FOREIGN_SCRIPT, Some("acme-audit")),
        _ => entry("*", FOREIGN_LOCAL_HTTP, None),
    }
}

fn options(precompact_memory_export: bool) -> IntegrationInstallOptions {
    IntegrationInstallOptions {
        consent: true,
        precompact_memory_export,
        repair: true,
    }
}

fn command_of(entry: &Json) -> &str {
    entry
        .pointer("/hooks/0/command")
        .and_then(Json::as_str)
        .expect("hook entry carries a command")
}

#[test]
fn current_unmarked_codeterm_hook_is_owned() {
    for event in CLAUDE_EVENTS {
        assert!(
            is_codeterm_hook(&current_unmarked_entry(event)),
            "{event}: the installed unmarked form must still be recognised as ours"
        );
    }

    // The two events the provider asymmetry hides behind, named explicitly.
    assert!(is_codeterm_hook(&current_unmarked_entry(
        "PermissionRequest"
    )));
    assert!(is_codeterm_hook(&current_unmarked_entry("Stop")));

    // Narrowly bounded: the same runtime variables without one of our exact
    // routes is not ours, so the fallback cannot grow into a shape heuristic.
    let off_route = entry(
        "*",
        r#"curl -s "$CODETERM_API_URL/api/plugins" -H "Authorization: Bearer $CODETERM_API_TOKEN" -H "X-Tab: $CODETERM_TAB_ID""#,
        None,
    );
    assert!(!is_codeterm_hook(&off_route));
}

#[test]
fn legacy_v2_codeterm_hook_is_owned() {
    let marked = entry("", LEGACY_V2_SESSION_START, Some("codeterm-hook-v2"));
    assert!(is_codeterm_hook(&marked), "exact ROG v2 entry is ours");

    // The same entry after a settings rewrite dropped `_source`.
    assert!(is_codeterm_hook(&entry("", LEGACY_V2_SESSION_START, None)));
    assert!(is_codeterm_hook(&entry("", LEGACY_V2_STOP, None)));

    // The generation before the versioned marker.
    assert!(is_codeterm_hook(&entry(
        "",
        LEGACY_V2_SESSION_START,
        Some("codeterm")
    )));
}

#[test]
fn foreign_hook_is_not_owned() {
    assert!(!is_codeterm_hook(&entry("*", FOREIGN_LOCAL_HTTP, None)));
    assert!(!is_codeterm_hook(&entry(
        "",
        FOREIGN_SCRIPT,
        Some("acme-audit")
    )));
    // A local port plus one of our route words is not ownership evidence.
    assert!(!is_codeterm_hook(&entry(
        "*",
        "curl http://127.0.0.1:7685/api/tabs/abc/hook/stop",
        None
    )));
    assert!(!is_codeterm_hook(&serde_json::json!({ "matcher": "*" })));
}

#[test]
fn reconcile_current_hooks_twice_keeps_one_entry_per_event() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let mut hooks = serde_json::Map::new();
    for event in CLAUDE_EVENTS {
        // Legacy and marked variants of the same event must collapse into the
        // one canonical entry too, not stack next to the current form.
        let mut entries = vec![foreign_entry(event), current_unmarked_entry(event)];
        if event == "SessionStart" {
            entries.push(entry("", LEGACY_V2_SESSION_START, Some("codeterm-hook-v2")));
        }
        if event == "Stop" {
            entries.push(entry("", LEGACY_V2_STOP, None));
        }
        hooks.insert(event.to_string(), Json::Array(entries));
    }
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&serde_json::json!({
            "model": "opus",
            "hooks": Json::Object(hooks),
        }))
        .unwrap(),
    )
    .unwrap();

    install_integration_at(&path, &options(true)).unwrap();
    let after_first = std::fs::read_to_string(&path).unwrap();
    install_integration_at(&path, &options(true)).unwrap();
    let after_second = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        after_first, after_second,
        "a second reconciliation over the reconciled set must write nothing new"
    );

    let settings: Json = serde_json::from_str(&after_second).unwrap();
    assert_eq!(settings["model"], "opus", "unrelated settings untouched");
    for event in CLAUDE_EVENTS {
        let entries = settings
            .pointer(&format!("/hooks/{event}"))
            .and_then(Json::as_array)
            .unwrap_or_else(|| panic!("{event} present"));
        let owned: Vec<&Json> = entries.iter().filter(|e| is_codeterm_hook(e)).collect();
        assert_eq!(owned.len(), 1, "{event}: exactly one CodeTerm entry");

        let command = command_of(owned[0]);
        assert!(
            command.contains(CODETERM_HOOK_MARKER_HEADER),
            "{event}: the canonical entry carries the durable marker: {command}"
        );
        assert!(
            !command.contains("localhost")
                && !command.contains("7685")
                && !command.contains("7686")
                && !command.contains("CODETERM_API_PORT")
                && !command.contains("CODETERM_PANE_ID"),
            "{event}: instance-agnostic, no port or pane identity: {command}"
        );
        assert!(
            command.contains("$CODETERM_API_TOKEN") && !command.contains("Bearer ct_"),
            "{event}: the token is read from the environment, never embedded: {command}"
        );

        let foreign: Vec<&Json> = entries.iter().filter(|e| !is_codeterm_hook(e)).collect();
        assert_eq!(foreign.len(), 1, "{event}: the foreign entry survives");
        assert_eq!(
            command_of(foreign[0]),
            command_of(&foreign_entry(event)),
            "{event}: the foreign entry is byte-identical"
        );
    }

    // The provider's own report agrees that the contract is now current, which
    // is what stops the next daemon start from writing again.
    let events_with_marker = CLAUDE_EVENTS
        .iter()
        .filter(|event| {
            settings
                .pointer(&format!("/hooks/{event}"))
                .and_then(Json::as_array)
                .is_some_and(|entries| {
                    entries.iter().any(|e| {
                        is_codeterm_hook(e) && command_of(e).contains(CODETERM_HOOK_MARKER_HEADER)
                    })
                })
        })
        .count();
    assert_eq!(events_with_marker, CLAUDE_EVENTS.len());
}
