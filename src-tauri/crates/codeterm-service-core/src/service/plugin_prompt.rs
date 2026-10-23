//! What core plugins tell every agent: the `Built-in plugins:` launch-prompt
//! section and the guides `domios docs <plugin-id>` falls back to.

use codeterm_plugin::agent_prompt_lines::{compose_built_in_plugins_section, PluginPromptLine};
use serde_json::{json, Value};

use crate::agent_commands::AgentCommandRegistry;

// Launch prompts are composed by free functions far from the service, so the registry view is snapshotted here.
static BUILT_IN_PLUGINS_SECTION: parking_lot::RwLock<Option<String>> =
    parking_lot::const_rwlock(None);

static CAPABILITY_HINTS: parking_lot::RwLock<
    Vec<super::general_agent::capability_hints::Capability>,
> = parking_lot::const_rwlock(Vec::new());

pub(crate) fn capability_hints() -> Vec<super::general_agent::capability_hints::Capability> {
    CAPABILITY_HINTS.read().clone()
}

pub fn refresh(registry: &AgentCommandRegistry) {
    *CAPABILITY_HINTS.write() = hint_capabilities(registry);
    *BUILT_IN_PLUGINS_SECTION.write() = compose_built_in_plugins_section(&prompt_lines(registry));
}

pub fn built_in_plugins_section() -> Option<String> {
    BUILT_IN_PLUGINS_SECTION.read().clone()
}

fn hint_capabilities(
    registry: &AgentCommandRegistry,
) -> Vec<super::general_agent::capability_hints::Capability> {
    crate::service::plugin_skills::granted_catalogs(registry)
        .into_iter()
        .map(
            |catalog| super::general_agent::capability_hints::Capability {
                purpose: catalog
                    .description
                    .or(catalog.prompt_line)
                    .unwrap_or(catalog.display_name),
                id: catalog.id,
            },
        )
        .collect()
}

fn prompt_lines(registry: &AgentCommandRegistry) -> Vec<PluginPromptLine> {
    registry
        .active()
        .iter()
        .filter_map(|handler| {
            let catalog = handler.catalog()?;
            Some(PluginPromptLine {
                id: catalog.id,
                core: catalog.core,
                enabled: handler.granted(),
                line: catalog.prompt_line,
            })
        })
        .collect()
}

/// Agent guides of enabled core plugins, by plugin id.
pub fn core_plugin_guides(registry: &AgentCommandRegistry) -> Value {
    let guides: Vec<Value> = crate::service::plugin_skills::granted_catalogs(registry)
        .into_iter()
        .filter(|catalog| catalog.is_core())
        .filter_map(|catalog| {
            let guide = catalog.guide?;
            Some(json!({
                "id": catalog.id,
                "display_name": catalog.display_name,
                "description": catalog.description,
                "guide": guide,
            }))
        })
        .collect();
    json!({ "guides": guides })
}

#[cfg(test)]
mod tests;
