use std::sync::Arc;
use std::{fs, path::Path};

use codeterm_plugin::agent_verbs::{AgentVerbCatalog, PLUGIN_SKILL_SOURCE_PREFIX};
use codeterm_plugin::js_runtime::{catalog_from_manifest, PluginManifest};
use codeterm_plugin::plugin_index::{
    entries_from_manifest, plan_plugin_index_sync, ExistingPluginIndexEntry, PluginIndexEntry,
};

use crate::agent_commands::AgentCommandRegistry;
use crate::mem::{MemKind, MemManager, MemScope, NewMemItem};

const PLUGIN_SKILL_IMPORTANCE: f64 = 6.0;

static SYNC_LOCK: parking_lot::Mutex<()> = parking_lot::const_mutex(());

pub fn granted_catalogs(registry: &AgentCommandRegistry) -> Vec<AgentVerbCatalog> {
    let mut catalogs: Vec<AgentVerbCatalog> = registry
        .active()
        .iter()
        .filter(|handler| handler.granted())
        .filter_map(|handler| handler.catalog())
        .collect();
    catalogs.sort_by(|a, b| a.id.cmp(&b.id));
    catalogs
}

fn to_new_item(item: PluginIndexEntry) -> NewMemItem {
    NewMemItem {
        kind: MemKind::Skill,
        scope: MemScope::Global,
        title: item.title,
        summary: Some(item.summary),
        area: None,
        tags: item.tags,
        body: item.body,
        source: Some(item.source),
        importance: PLUGIN_SKILL_IMPORTANCE,
        source_path: None,
        pinned: false,
    }
}

pub fn sync(mem: &MemManager) {
    crate::mem::store::with_default_system_author("plugin-skills", || {
        let _serial = SYNC_LOCK.lock();
        let desired = match installed_plugin_entries() {
            Ok(entries) => entries,
            Err(error) => {
                log::warn!("[plugin-index] load installed manifests failed: {error}");
                return;
            }
        };
        let existing =
            match mem.list_by_source_prefix(MemKind::Skill.as_str(), PLUGIN_SKILL_SOURCE_PREFIX) {
                Ok(items) => items
                    .into_iter()
                    .map(|item| ExistingPluginIndexEntry {
                        mem_id: item.id,
                        title: item.title,
                        source: item.source.unwrap_or_default(),
                        body: item.body,
                    })
                    .collect::<Vec<_>>(),
                Err(error) => {
                    log::warn!("[plugin-index] list existing items failed: {error}");
                    return;
                }
            };
        let plan = plan_plugin_index_sync(&desired, &existing);
        for item in plan.upserts {
            let plugin_id = item.plugin_id.clone();
            match mem.save_with_outcome(to_new_item(item), true) {
                Ok(outcome) => log::info!(
                    "[plugin-index] {plugin_id}: {} {}",
                    outcome.outcome,
                    outcome.id
                ),
                Err(error) => log::warn!("[plugin-index] {plugin_id}: save failed: {error}"),
            }
        }
        for id in plan.removals {
            match mem.delete(&id) {
                Ok(_) => log::info!("[plugin-index] removed {id}"),
                Err(error) => log::warn!("[plugin-index] remove {id} failed: {error}"),
            }
        }
    })
}

/// Off the caller's thread: mem writes block, and callers include the async runtime.
pub fn spawn_sync(mem: Arc<MemManager>) {
    crate::async_runtime::spawn_blocking(move || sync(&mem));
}

fn installed_plugin_entries() -> Result<Vec<PluginIndexEntry>, String> {
    let root = codeterm_paths::data_dir().join("plugins");
    if !root.exists() {
        return Ok(Vec::new());
    }
    let entries =
        fs::read_dir(&root).map_err(|error| format!("read {}: {error}", root.display()))?;
    let mut indexed = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("read plugin entry: {error}"))?;
        if !entry
            .file_type()
            .map_err(|error| format!("inspect {}: {error}", entry.path().display()))?
            .is_dir()
        {
            continue;
        }
        let manifest_path = entry.path().join("plugin.json");
        if !manifest_path.exists() {
            continue;
        }
        let manifest = match PluginManifest::load(&manifest_path) {
            Ok(manifest) => manifest,
            Err(error) => {
                log::warn!("[plugin-index] skip {}: {error}", manifest_path.display());
                continue;
            }
        };
        let catalog = catalog_from_manifest(&manifest, false);
        let settings_schema = load_settings_schema(&manifest);
        indexed.extend(entries_from_manifest(
            &manifest,
            settings_schema.as_ref(),
            catalog.guide.as_deref(),
        ));
    }
    Ok(indexed)
}

fn load_settings_schema(manifest: &PluginManifest) -> Option<serde_json::Value> {
    let relative = manifest.settings_schema.as_deref()?;
    let path = Path::new(relative);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return None;
    }
    let bundle = fs::canonicalize(manifest.source_path.as_deref()?.parent()?).ok()?;
    let schema = fs::canonicalize(bundle.join(path)).ok()?;
    if !schema.starts_with(&bundle) {
        return None;
    }
    let contents = fs::read_to_string(schema).ok()?;
    serde_json::from_str(&contents).ok()
}

#[cfg(test)]
mod tests;
