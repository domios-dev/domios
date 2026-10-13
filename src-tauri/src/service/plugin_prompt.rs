//! What core plugins tell every agent: the `Built-in plugins:` launch-prompt
//! section and the guides `codeterm docs <plugin-id>` falls back to.

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
mod tests {
    use std::sync::Arc;

    use codeterm_plugin::agent_verbs::AgentVerbCatalog;

    use super::*;
    use crate::agent_commands::{AgentCommandProvider, AgentCommandResult};

    struct Fake {
        catalog: AgentVerbCatalog,
        granted: bool,
    }

    impl AgentCommandProvider for Fake {
        fn id(&self) -> &str {
            &self.catalog.id
        }
        fn granted(&self) -> bool {
            self.granted
        }
        fn catalog(&self) -> Option<AgentVerbCatalog> {
            Some(self.catalog.clone())
        }
        fn on_agent_command(
            &self,
            _: &str,
            _: &str,
            _: &[String],
        ) -> Result<AgentCommandResult, String> {
            Err("unused".into())
        }
    }

    fn registry(plugins: &[(&str, bool, bool)]) -> AgentCommandRegistry {
        let registry = AgentCommandRegistry::new();
        for (id, core, granted) in plugins {
            let catalog = AgentVerbCatalog::compose(id, "Opaque", None, &[], None, &[])
                .with_guide(Some(format!("GUIDE_{id}")))
                .with_prompt_line(*core, Some(format!("LINE_{id}")));
            registry.register(
                id.to_string(),
                Arc::new(Fake {
                    catalog,
                    granted: *granted,
                }),
            );
        }
        registry
    }

    #[test]
    fn hint_catalog_uses_the_same_granted_inventory_as_capabilities() {
        let registry = registry(&[
            ("core-q7", true, true),
            ("cap-q7", false, true),
            ("off-q7", true, false),
        ]);
        let expected = crate::service::plugin_skills::granted_catalogs(&registry);
        let hints = hint_capabilities(&registry);
        assert_eq!(
            hints
                .iter()
                .map(|hint| hint.id.as_str())
                .collect::<Vec<_>>(),
            expected
                .iter()
                .map(|catalog| catalog.id.as_str())
                .collect::<Vec<_>>()
        );
        for (hint, catalog) in hints.iter().zip(expected) {
            assert_eq!(hint.purpose, catalog.prompt_line.unwrap());
        }
    }

    #[test]
    fn only_granted_core_plugins_reach_the_section_and_the_guides() {
        let registry = registry(&[
            ("zcore-q7", true, true),
            ("acore-q7", true, true),
            ("third-q7", false, true),
            ("off-q7", true, false),
        ]);
        assert_eq!(
            compose_built_in_plugins_section(&prompt_lines(&registry)).as_deref(),
            Some("Built-in plugins:\n- LINE_acore-q7\n- LINE_zcore-q7")
        );
        let ids: Vec<String> = core_plugin_guides(&registry)["guides"]
            .as_array()
            .unwrap()
            .iter()
            .map(|guide| guide["id"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(ids, ["acore-q7", "zcore-q7"]);
        assert_eq!(
            compose_built_in_plugins_section(&prompt_lines(&AgentCommandRegistry::new())),
            None
        );
    }
}
