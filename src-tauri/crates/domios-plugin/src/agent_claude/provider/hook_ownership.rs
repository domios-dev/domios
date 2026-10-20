//! Who owns a Claude hook entry.
//!
//! The durable identity is an exact marker carried in the command. Ownership
//! is limited to the current generation and the immediately previous release.

use serde_json::Value as Json;

mod upgrade_from_1_14_9;

pub const DOMIOS_HOOK_MARKER: &str = "domios-hook-v4";
pub const DOMIOS_HOOK_MARKER_HEADER: &str = "X-Domios-Hook: domios-hook-v4";

pub fn marker_header_arg() -> String {
    format!("-H \"{DOMIOS_HOOK_MARKER_HEADER}\"")
}

pub fn command_carries_marker(command: &str) -> bool {
    command.contains(DOMIOS_HOOK_MARKER_HEADER)
}

fn command_is_owned(command: &str) -> bool {
    command_carries_marker(command) || upgrade_from_1_14_9::command_is_owned(command)
}

fn hook_is_owned(hook: &Json) -> bool {
    hook.get("command")
        .and_then(Json::as_str)
        .is_some_and(command_is_owned)
}

pub fn is_domios_hook(entry: &Json) -> bool {
    entry
        .get("hooks")
        .and_then(Json::as_array)
        .is_some_and(|hooks| hooks.iter().any(hook_is_owned))
}

pub fn entry_is_1_14_9_hook(entry: &Json) -> bool {
    entry
        .get("hooks")
        .and_then(Json::as_array)
        .is_some_and(|hooks| {
            hooks.iter().any(|hook| {
                hook.get("command")
                    .and_then(Json::as_str)
                    .is_some_and(upgrade_from_1_14_9::command_is_owned)
            })
        })
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

pub fn entries_carry_marker(entries: &[Json]) -> bool {
    entries
        .iter()
        .filter(|entry| is_domios_hook(entry))
        .all(entry_carries_marker)
}
