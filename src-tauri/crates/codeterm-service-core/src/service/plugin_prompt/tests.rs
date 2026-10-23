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
        _: Option<&str>,
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
