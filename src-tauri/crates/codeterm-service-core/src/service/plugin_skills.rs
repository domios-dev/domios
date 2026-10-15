//! Keeps one mem skill item per agent-command plugin, so `codeterm mem search`
//! finds what installed plugins let agents do.

use std::sync::Arc;

use codeterm_plugin::agent_verbs::{
    plan_plugin_skill_sync, AgentVerbCatalog, ExistingPluginSkill, PluginSkillItem,
    PLUGIN_SKILL_SOURCE_PREFIX,
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

fn to_new_item(item: PluginSkillItem) -> NewMemItem {
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

pub fn sync(mem: &MemManager, registry: &AgentCommandRegistry) {
    let _serial = SYNC_LOCK.lock();
    let existing =
        match mem.list_by_source_prefix(MemKind::Skill.as_str(), PLUGIN_SKILL_SOURCE_PREFIX) {
            Ok(items) => items
                .into_iter()
                .map(|item| ExistingPluginSkill {
                    mem_id: item.id,
                    title: item.title,
                    source: item.source.unwrap_or_default(),
                    body: item.body,
                })
                .collect::<Vec<_>>(),
            Err(error) => {
                log::warn!("[plugin-skills] list existing items failed: {error}");
                return;
            }
        };
    let plan = plan_plugin_skill_sync(&granted_catalogs(registry), &existing);
    for item in plan.upserts {
        let plugin_id = item.plugin_id.clone();
        match mem.save_with_outcome(to_new_item(item), true) {
            Ok(outcome) => log::info!(
                "[plugin-skills] {plugin_id}: {} {}",
                outcome.outcome,
                outcome.id
            ),
            Err(error) => log::warn!("[plugin-skills] {plugin_id}: save failed: {error}"),
        }
    }
    for id in plan.removals {
        match mem.delete(&id) {
            Ok(_) => log::info!("[plugin-skills] removed {id}"),
            Err(error) => log::warn!("[plugin-skills] remove {id} failed: {error}"),
        }
    }
}

/// Off the caller's thread: mem writes block, and callers include the async runtime.
pub fn spawn_sync(mem: Arc<MemManager>, registry: Arc<AgentCommandRegistry>) {
    crate::async_runtime::spawn_blocking(move || sync(&mem, &registry));
}
