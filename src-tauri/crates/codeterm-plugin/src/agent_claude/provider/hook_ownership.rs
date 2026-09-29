//! Who owns a Claude hook entry.
//!
//! Claude Code rewrites `~/.claude/settings.json` and its serializer drops the
//! sibling `_source` key, so the durable identity is a CodeTerm header carried
//! inside the command itself. The two command fingerprints below are migration
//! compatibility for entries written before that header existed: each is bound
//! to an exact string CodeTerm generated, never to a route or port shape.

use serde_json::Value as Json;

pub const CODETERM_HOOK_MARKER: &str = "codeterm-hook-v3";

/// The durable identity. Inert, instance-agnostic, and carries no host, port,
/// token or other connection credential.
pub const CODETERM_HOOK_MARKER_HEADER: &str = "X-CodeTerm-Hook: codeterm-hook-v3";

/// The curl argument every generated hook command carries.
pub fn marker_header_arg() -> String {
    format!("-H \"{CODETERM_HOOK_MARKER_HEADER}\"")
}

/// Exact route literals the current generator emits. The unmarked-form
/// fingerprint accepts nothing outside this list.
const CURRENT_ROUTES: [&str; 4] = [
    "/api/tabs/$CODETERM_TAB_ID/hook/permission",
    "/api/tabs/$CODETERM_TAB_ID/hook/stop",
    "/api/tabs/$CODETERM_TAB_ID/hook/session_start",
    "$CODETERM_API_URL/api/mem/flush",
];

pub fn command_carries_marker(command: &str) -> bool {
    command.contains(CODETERM_HOOK_MARKER_HEADER)
}

fn source_is_codeterm(source: &str) -> bool {
    source.starts_with("codeterm-hook") || source == "codeterm"
}

/// Migration compatibility only: the current command shape as it exists on
/// machines installed before the marker. All three runtime variables plus one
/// exact generated route must be present.
fn command_is_current_unmarked(command: &str) -> bool {
    command.contains("$CODETERM_API_URL")
        && command.contains("$CODETERM_TAB_ID")
        && command.contains("$CODETERM_API_TOKEN")
        && CURRENT_ROUTES.iter().any(|route| command.contains(route))
}

/// Migration compatibility only: the pane-era v2 command, whose `_source` a
/// later settings rewrite may already have removed.
fn command_is_legacy_v2(command: &str) -> bool {
    command.contains("$CODETERM_PANE_ID")
        && (command.contains("/api/panes/$CODETERM_PANE_ID/hook/")
            || command.contains("/api/mem/flush"))
}

fn hook_is_codeterm(hook: &Json) -> bool {
    if hook
        .get("_source")
        .and_then(Json::as_str)
        .is_some_and(source_is_codeterm)
    {
        return true;
    }
    hook.get("command")
        .and_then(Json::as_str)
        .is_some_and(|command| {
            command_carries_marker(command)
                || command_is_current_unmarked(command)
                || command_is_legacy_v2(command)
        })
}

/// True when CodeTerm installed this entry at any version. Install removes
/// every owned variant before writing the canonical one, so an owned entry is
/// replaced rather than duplicated; anything else is foreign and preserved.
pub fn is_codeterm_hook(entry: &Json) -> bool {
    entry
        .get("hooks")
        .and_then(Json::as_array)
        .is_some_and(|hooks| hooks.iter().any(hook_is_codeterm))
}

pub fn entry_carries_marker(entry: &Json) -> bool {
    entry
        .get("hooks")
        .and_then(Json::as_array)
        .is_some_and(|hooks| {
            hooks.iter().any(|hook| {
                hook.get("command")
                    .and_then(Json::as_str)
                    .is_some_and(command_carries_marker)
            })
        })
}

/// Every owned entry in this event carries the durable marker. An event with
/// no owned entry is vacuously current — absence is the install path's problem.
pub fn entries_carry_marker(entries: &[Json]) -> bool {
    entries
        .iter()
        .filter(|entry| is_codeterm_hook(entry))
        .all(entry_carries_marker)
}
