use super::*;
use std::collections::BTreeSet;

pub(super) fn system_plugins_to_retire(
    installed: &BTreeSet<String>,
    current_channel: &BTreeSet<String>,
) -> Vec<String> {
    installed.difference(current_channel).cloned().collect()
}

pub(super) fn retire_missing_official_plugins(
    current_ids: &[String],
) -> Result<Vec<String>, String> {
    let current_channel: BTreeSet<String> = current_ids.iter().cloned().collect();
    let mut installed_official = BTreeSet::new();
    for id in installed_ids()? {
        let Ok(record) = read_install_record(&id) else {
            continue;
        };
        if record.id != id
            || !matches!(
                record.source,
                InstallSource::SystemChannel { ref channel, .. }
                    if channel == SYSTEM_CHANNEL_NAME
            )
        {
            continue;
        }
        installed_official.insert(id);
    }

    let mut retired = Vec::new();
    for id in system_plugins_to_retire(&installed_official, &current_channel) {
        let parsed_id = PluginId::parse(id.clone())?;
        let dir = plugin_dir(&parsed_id);
        ensure_plugin_path_inside(&dir)?;
        if !dir.exists() {
            continue;
        }
        let before = manifest_version(&dir).ok().flatten();
        let result = fs::remove_dir_all(&dir).map_err(|error| {
            format!(
                "retire official plugin '{id}' at {}: {error}",
                dir.display()
            )
        });
        audit_plugin_result(
            "retire",
            Some(&id),
            Some(SYSTEM_CHANNEL_NAME),
            before,
            None,
            &result,
        );
        result?;
        retired.push(id);
    }
    Ok(retired)
}

#[cfg(test)]
mod tests;
