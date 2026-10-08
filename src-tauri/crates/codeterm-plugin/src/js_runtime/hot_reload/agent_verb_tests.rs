use super::tests::test_caps;
use super::*;

const PLUGIN_ID: &str = "verb-fixture";
const VERB: &str = "zz-alpha <target-q7>";
const KEYWORD: &str = "kw-q7";

fn manifest() -> PluginManifest {
    serde_json::from_str(&format!(
        r#"{{"id":"{PLUGIN_ID}","version":"0","displayName":"Verb fixture",
            "capabilities":{{}},"agentVerbs":["{VERB}"],"agentKeywords":["{KEYWORD}"]}}"#
    ))
    .expect("manifest")
}

fn vm() -> Arc<super::super::PluginVm> {
    Arc::new(
        super::super::PluginVm::from_source(
            PLUGIN_ID,
            r#"module.exports.default = { onAgentCommand() { return { result: "" }; } };"#
                .to_string(),
        )
        .expect("vm"),
    )
}

#[test]
fn registered_handler_publishes_the_manifest_verbs_until_deregistered() {
    let caps = test_caps();
    caps.register_plugin(&manifest(), &vm());

    let catalog = caps
        .agent_command
        .get(PLUGIN_ID)
        .and_then(|handler| handler.catalog())
        .expect("a registered agent-command plugin carries its catalog");
    assert_eq!(catalog.id, PLUGIN_ID);
    assert_eq!(catalog.verbs, vec![VERB.to_string()]);
    assert!(catalog.keywords.iter().any(|k| k == KEYWORD));

    caps.deregister_plugin(PLUGIN_ID);
    assert!(caps.agent_command.get(PLUGIN_ID).is_none());
}

#[test]
fn registered_handler_carries_the_bundle_agent_guide() {
    const GUIDE_TOKEN: &str = "guide-token-q7";
    let bundle = tempfile::tempdir().expect("bundle");
    std::fs::write(bundle.path().join("AGENT.md"), GUIDE_TOKEN).expect("guide");
    let mut guided = manifest();
    guided.agent_guide = Some("AGENT.md".to_string());
    guided.source_path = Some(bundle.path().join("plugin.json"));
    let mut escaping = guided.clone();
    escaping.agent_guide = Some("../AGENT.md".to_string());

    let caps = test_caps();
    caps.register_plugin(&guided, &vm());
    let catalog = caps
        .agent_command
        .get(PLUGIN_ID)
        .and_then(|handler| handler.catalog())
        .expect("catalog");
    assert_eq!(catalog.guide.as_deref(), Some(GUIDE_TOKEN));

    caps.deregister_plugin(PLUGIN_ID);
    caps.register_plugin(&escaping, &vm());
    let catalog = caps
        .agent_command
        .get(PLUGIN_ID)
        .and_then(|handler| handler.catalog())
        .expect("catalog");
    assert!(catalog.guide.is_none());
}
