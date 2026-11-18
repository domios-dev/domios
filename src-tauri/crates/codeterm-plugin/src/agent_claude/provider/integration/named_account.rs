use super::super::hook_ownership::command_carries_marker;
use super::{hook_entry, hook_event_roster, Json};
use crate::provider::HookIngestionRoute;
use std::{
    fs,
    io::{self, Read},
    path::Path,
};

const MAX_SETTINGS_BYTES: u64 = 256 * 1024;

fn read_settings(path: &Path) -> Result<Option<String>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.is_file() || metadata.len() > MAX_SETTINGS_BYTES {
        return Err("Named account settings must be a bounded regular file".into());
    }
    let mut contents = String::new();
    fs::File::open(path)
        .and_then(|file| {
            file.take(MAX_SETTINGS_BYTES + 1)
                .read_to_string(&mut contents)
        })
        .map_err(|error| error.to_string())?;
    if contents.len() as u64 > MAX_SETTINGS_BYTES {
        return Err("Named account settings exceed their size bound".into());
    }
    Ok(Some(contents))
}

fn marked_command(hook: &Json) -> bool {
    hook.get("command")
        .and_then(Json::as_str)
        .is_some_and(command_carries_marker)
}

pub(super) fn remove_owned(entries: &mut Vec<Json>) -> bool {
    let mut changed = false;
    entries.retain_mut(|entry| {
        let Some(hooks) = entry.get_mut("hooks").and_then(Json::as_array_mut) else {
            return true;
        };
        let before = hooks.len();
        hooks.retain(|hook| !marked_command(hook));
        changed |= hooks.len() != before;
        hooks.len() == before || !hooks.is_empty()
    });
    changed
}

fn has_exact_hook(entries: &[Json], expected: &Json) -> bool {
    let mut owned = entries.iter().flat_map(|entry| {
        entry
            .get("hooks")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter(|hook| marked_command(hook))
            .map(|hook| (&entry["matcher"], hook))
    });
    let Some((matcher, hook)) = owned.next() else {
        return false;
    };
    let mut hook = hook.clone();
    let mut expected_hook = expected["hooks"][0].clone();
    // The provider strips the legacy sibling source field when it saves settings.
    for value in [&mut hook, &mut expected_hook] {
        if let Some(object) = value.as_object_mut() {
            object.remove("_source");
        }
    }
    matcher == &expected["matcher"] && hook == expected_hook && owned.next().is_none()
}

pub(crate) fn uninstall(home: &Path) -> Result<(), String> {
    let path = home.join("settings.json");
    let Some(original) = read_settings(&path)? else {
        return Ok(());
    };
    let mut settings: Json =
        serde_json::from_str(&original).map_err(|_| "Named account settings are malformed")?;
    let root = settings
        .as_object_mut()
        .ok_or("Named account settings must be an object")?;
    let Some(hooks) = root.get_mut("hooks") else {
        return Ok(());
    };
    let hooks = hooks
        .as_object_mut()
        .ok_or("Named account hooks must be an object")?;
    let mut changed = false;
    for entries in hooks.values_mut().filter_map(Json::as_array_mut) {
        changed |= remove_owned(entries);
    }
    if changed {
        replace_settings(&path, Some(&original), &settings)?;
    }
    Ok(())
}

fn replace_settings(path: &Path, original: Option<&str>, settings: &Json) -> Result<(), String> {
    if read_settings(path)?.as_deref() != original {
        return Err("Named account settings changed during hook update".into());
    }
    let bytes = serde_json::to_vec_pretty(settings).map_err(|error| error.to_string())?;
    codeterm_util::private_file::write_atomic(path, &bytes).map_err(|error| error.to_string())
}

pub(crate) fn session_hook(home: &Path, repair_consented: bool) -> Result<bool, String> {
    let path = home.join("settings.json");
    let original = read_settings(&path)?;
    let mut settings = match original.as_deref() {
        Some(contents) => serde_json::from_str::<Json>(contents)
            .map_err(|_| "Named account settings are malformed")?,
        None => serde_json::json!({}),
    };
    let root = settings
        .as_object_mut()
        .ok_or("Named account settings must be an object")?;
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or("Named account hooks must be an object")?;
    let mut current_session_hook = false;
    let mut changed = false;
    for binding in hook_event_roster() {
        let Some(expected) = hook_entry(&binding) else {
            continue;
        };
        let entries = hooks
            .entry(binding.event)
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .ok_or("Named account hook entries must be arrays")?;
        let current = has_exact_hook(entries, &expected);
        if binding.route == HookIngestionRoute::SessionStart {
            current_session_hook = current || repair_consented;
        }
        if repair_consented && !current {
            remove_owned(entries);
            entries.push(expected);
            changed = true;
        }
    }
    if changed {
        replace_settings(&path, original.as_deref(), &settings)?;
    }
    Ok(current_session_hook)
}
