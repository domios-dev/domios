use super::*;
use std::collections::BTreeSet;
use std::sync::Mutex;

static RETIREMENT_LOCK: Mutex<()> = Mutex::new(());

pub(super) fn system_plugins_to_retire(
    installed: &BTreeSet<String>,
    current_channel: &BTreeSet<String>,
) -> Vec<String> {
    installed.difference(current_channel).cloned().collect()
}

pub(super) fn should_retire_plugin(
    directory_id: &str,
    manifest_id: Option<&str>,
    record: Option<&InstallRecord>,
    removed_system_ids: &BTreeSet<String>,
    retired_ids: &BTreeSet<String>,
) -> bool {
    if manifest_id != Some(directory_id) {
        return false;
    }

    match record {
        None => retired_ids.contains(directory_id),
        Some(record) if is_official_system_record(directory_id, record) => {
            retired_ids.contains(directory_id) || removed_system_ids.contains(directory_id)
        }
        Some(_) => false,
    }
}

pub(super) fn retire_missing_official_plugins(
    current_ids: &[String],
    retired_ids: &[String],
) -> Result<Vec<String>, String> {
    let _guard = RETIREMENT_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let current_channel: BTreeSet<String> = current_ids.iter().cloned().collect();
    let retired_ids = validate_retired_ids(&current_channel, retired_ids)?;
    let mut candidates = Vec::new();
    let mut installed_official = BTreeSet::new();

    for id in installed_ids()? {
        let Ok(parsed_id) = PluginId::parse(id.clone()) else {
            continue;
        };
        let dir = plugin_dir(&parsed_id);
        if !dir.exists() || ensure_plugin_path_inside(&dir).is_err() {
            continue;
        }
        let record = read_install_record_if_present(&id);
        if record.as_ref().is_ok_and(|record| {
            record
                .as_ref()
                .is_some_and(|record| is_official_system_record(&id, record))
        }) {
            installed_official.insert(id.clone());
        }
        candidates.push(InstalledPlugin { id, dir, record });
    }

    let removed_system_ids: BTreeSet<String> =
        system_plugins_to_retire(&installed_official, &current_channel)
            .into_iter()
            .collect();
    let mut retired = Vec::new();

    for candidate in candidates {
        let record = match candidate.record {
            Ok(record) => record,
            Err(error) => {
                if retired_ids.contains(&candidate.id) {
                    audit_retirement_skip(
                        "retire-preserved-unreadable-record",
                        &candidate.id,
                        Some(error),
                    );
                }
                continue;
            }
        };
        if !retired_ids.contains(&candidate.id) && !removed_system_ids.contains(&candidate.id) {
            continue;
        }

        let manifest_id = manifest_id(&candidate.dir).ok();
        if !should_retire_plugin(
            &candidate.id,
            manifest_id.as_deref(),
            record.as_ref(),
            &removed_system_ids,
            &retired_ids,
        ) {
            if retired_ids.contains(&candidate.id)
                && record
                    .as_ref()
                    .is_some_and(|record| !is_official_system_record(&candidate.id, record))
            {
                audit_retirement_skip("retire-preserved-install-record", &candidate.id, None);
            }
            continue;
        }

        let before = manifest_version(&candidate.dir).ok().flatten();
        let result = fs::remove_dir_all(&candidate.dir).map_err(|error| {
            format!(
                "retire official plugin '{}' at {}: {error}",
                candidate.id,
                candidate.dir.display()
            )
        });
        audit_plugin_result(
            "retire",
            Some(&candidate.id),
            Some(SYSTEM_CHANNEL_NAME),
            before,
            None,
            &result,
        );
        result?;
        retired.push(candidate.id);
    }

    retired.sort();
    Ok(retired)
}

fn validate_retired_ids(
    current_ids: &BTreeSet<String>,
    retired_ids: &[String],
) -> Result<BTreeSet<String>, String> {
    let mut validated = BTreeSet::new();
    for id in retired_ids {
        PluginId::parse(id.clone())?;
        if !is_reserved_system_id(id) {
            return Err(format!(
                "retired system plugin id '{id}' is not reserved by the embedded official set"
            ));
        }
        if current_ids.contains(id) {
            return Err(format!(
                "system plugin '{id}' cannot be both current and retired"
            ));
        }
        if !validated.insert(id.clone()) {
            return Err(format!("duplicate retired system plugin id '{id}'"));
        }
    }
    Ok(validated)
}

fn is_official_system_record(id: &str, record: &InstallRecord) -> bool {
    record.id == id
        && matches!(
            &record.source,
            InstallSource::SystemChannel { channel, .. } if channel == SYSTEM_CHANNEL_NAME
        )
}

fn audit_retirement_skip(kind: &str, id: &str, error: Option<String>) {
    let event = PluginEvent {
        ts: now_rfc3339(),
        kind: kind.to_string(),
        actor: "plugin-host".to_string(),
        ok: error.is_none(),
        plugin: Some(id.to_string()),
        channel: Some(SYSTEM_CHANNEL_NAME.to_string()),
        from: None,
        to: None,
        version: None,
        error,
    };
    let _ = append_audit_event(&event);
}

struct InstalledPlugin {
    id: String,
    dir: PathBuf,
    record: Result<Option<InstallRecord>, String>,
}

#[cfg(test)]
mod tests;
