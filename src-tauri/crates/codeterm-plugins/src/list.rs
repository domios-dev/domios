use super::*;

pub(super) fn list_installed() -> Result<serde_json::Value, String> {
    let root = plugins_root();
    fs::create_dir_all(&root).map_err(|e| format!("mkdir {}: {e}", root.display()))?;
    let mut rows = Vec::new();
    for entry in fs::read_dir(&root).map_err(|e| format!("read_dir {}: {e}", root.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            continue;
        }
        let path = entry.path();
        let folder_id = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        if folder_id.ends_with(".tmp-install") {
            continue;
        }
        let id = manifest_id(&path).unwrap_or_else(|_| folder_id.clone());
        let manifest = read_plugin_manifest(&path).ok();
        let record = read_install_record(&folder_id)
            .or_else(|_| read_install_record(&id))
            .ok();
        let provenance = plugin_provenance(
            &id,
            record.as_ref().map(|record| &record.source),
            manifest
                .as_ref()
                .and_then(|manifest| manifest.version.as_deref()),
        );
        let update = installed_update_status(
            &id,
            provenance.version.as_deref(),
            provenance.source.as_ref(),
        );
        rows.push(InstalledPluginRow {
            update,
            id: id.clone(),
            version: provenance.version,
            display_name: manifest.and_then(|m| m.display_name),
            path: path.display().to_string(),
            install_path: path.display().to_string(),
            install_time: record.as_ref().map(|r| r.installed_at.clone()),
            manifest_hash: manifest_hash(&path).ok(),
            verified: is_verified_install(&id, record.as_ref()),
            last_change: record.as_ref().and_then(|r| r.last_change.clone()),
            state: if path.with_extension("tmp-install").exists() {
                "mid-install".to_string()
            } else if record.is_none() && is_reserved_system_id(&id) {
                "bundled".to_string()
            } else {
                "installed".to_string()
            },
            source: provenance.source,
            system: provenance.system,
            removable: !provenance.system,
        });
    }
    merge_system_rows(&mut rows);
    Ok(serde_json::json!({ "plugins": rows }))
}

// when not extracted to disk (dev, or a fresh prod before bootstrap).
pub(super) fn merge_system_rows(rows: &mut Vec<InstalledPluginRow>) {
    let present: std::collections::BTreeSet<String> = rows.iter().map(|r| r.id.clone()).collect();
    for id in SYSTEM_PLUGIN_IDS {
        if !present.contains(id) {
            rows.push(virtual_system_row(id));
        }
    }
    rows.sort_by(|a, b| {
        system_rank(&a.id)
            .cmp(&system_rank(&b.id))
            .then_with(|| a.id.cmp(&b.id))
    });
}

pub(super) fn system_rank(id: &str) -> (u8, usize) {
    SYSTEM_PLUGIN_IDS
        .iter()
        .position(|s| *s == id)
        .map(|p| (0, p))
        .unwrap_or((1, 0))
}

pub(super) fn virtual_system_row(id: &str) -> InstalledPluginRow {
    let metadata = bundled_metadata(id).map(|(_, manifest)| manifest);
    let version = metadata
        .as_ref()
        .and_then(|manifest| manifest.version.clone());
    InstalledPluginRow {
        update: installed_update_status(id, version.as_deref(), None),
        id: id.to_string(),
        version,
        display_name: metadata.and_then(|manifest| manifest.display_name),
        path: String::new(),
        install_path: String::new(),
        install_time: None,
        manifest_hash: None,
        verified: true,
        last_change: None,
        state: "bundled".to_string(),
        source: effective_install_source(id, None),
        system: true,
        removable: false,
    }
}
