use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;

const MAX_HOOK_FILE_BYTES: u64 = 1024 * 1024;

struct HomeHook {
    env: &'static str,
    file: &'static str,
    marker: &'static str,
}

static HOME_HOOKS: [HomeHook; 2] = [
    HomeHook {
        env: "CLAUDE_CONFIG_DIR",
        file: "settings.json",
        marker: "codeterm-hook-v3",
    },
    HomeHook {
        env: "CODEX_HOME",
        file: "hooks.json",
        marker: "codeterm-codex-session-binding-v1",
    },
];

#[derive(Debug, PartialEq, Eq)]
enum HookObservation {
    Marked,
    Missing,
    Unmarked,
    Invalid,
    Unreadable,
    Oversized,
}

pub(super) fn schedule(
    tab_id: &str,
    launch_env: &HashMap<String, String>,
    inherited_env: &HashMap<String, String>,
) {
    let homes: Vec<_> = HOME_HOOKS
        .iter()
        .filter_map(|hook| {
            effective_home(launch_env, inherited_env, hook.env)
                .map(|(home, source)| (hook, PathBuf::from(home).join(hook.file), source))
        })
        .collect();
    if homes.is_empty() {
        return;
    }
    let tab_id = tab_id.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        for (hook, path, source) in homes {
            let observation = observe_hook_file(&path, hook.marker);
            if observation != HookObservation::Marked {
                log::warn!(
                    "[account-home-diagnostic] tab={} home_env={} source={} hook_file={:?} observation={:?}; session binding may be unavailable in this home; diagnostic is read-only",
                    tab_id,
                    hook.env,
                    source,
                    path,
                    observation,
                );
            }
        }
    });
}

fn effective_home<'a>(
    launch_env: &'a HashMap<String, String>,
    inherited_env: &'a HashMap<String, String>,
    name: &str,
) -> Option<(&'a str, &'static str)> {
    let (home, source) = match launch_env.get(name) {
        Some(home) => (home, "launch"),
        None => (inherited_env.get(name)?, "inherited"),
    };
    (!home.trim().is_empty()).then_some((home.as_str(), source))
}

fn observe_hook_file(path: &std::path::Path, marker: &str) -> HookObservation {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return HookObservation::Missing;
        }
        Err(_) => return HookObservation::Unreadable,
    };
    let mut bytes = Vec::new();
    if file
        .take(MAX_HOOK_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return HookObservation::Unreadable;
    }
    if bytes.len() as u64 > MAX_HOOK_FILE_BYTES {
        return HookObservation::Oversized;
    }
    classify_hook_document(&bytes, marker)
}

fn classify_hook_document(bytes: &[u8], marker: &str) -> HookObservation {
    let Ok(document) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return HookObservation::Invalid;
    };
    if !document.is_object() {
        return HookObservation::Invalid;
    }
    let marked = document
        .pointer("/hooks/SessionStart")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("hooks").and_then(serde_json::Value::as_array))
        .flatten()
        .filter(|hook| hook.get("type").and_then(serde_json::Value::as_str) == Some("command"))
        .filter_map(|hook| hook.get("command").and_then(serde_json::Value::as_str))
        .any(|command| command_carries_marker(command, marker));
    if marked {
        HookObservation::Marked
    } else {
        HookObservation::Unmarked
    }
}

fn command_carries_marker(command: &str, marker: &str) -> bool {
    [
        format!("X-CodeTerm-Hook: {marker}"),
        format!("X-CodeTerm-Hook:{marker}"),
    ]
    .iter()
    .any(|header| {
        command.match_indices(header.as_str()).any(|(start, _)| {
            command[start + header.len()..]
                .chars()
                .next()
                .is_none_or(|ch| ch.is_ascii_whitespace() || ch == '\'' || ch == '"')
        })
    })
}

#[cfg(test)]
mod tests;
