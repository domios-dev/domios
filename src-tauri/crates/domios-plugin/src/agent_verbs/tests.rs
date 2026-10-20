use super::*;

const VERB_A: &str = "zz-alpha";
const VERB_B: &str = "zz-beta <target-q7> [--flag <v>] <payload-q7>";

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn catalog(id: &str) -> AgentVerbCatalog {
    AgentVerbCatalog::compose(
        id,
        "Opaque Name",
        Some("opaque description"),
        &strings(&[VERB_A, VERB_B]),
        None,
        &strings(&["kw-one", "kw-two"]),
    )
}

fn stored(item: &PluginSkillItem, mem_id: &str) -> ExistingPluginSkill {
    ExistingPluginSkill {
        mem_id: mem_id.to_string(),
        title: item.title.clone(),
        source: item.source.clone(),
        body: item.body.clone(),
    }
}

#[test]
fn declared_verbs_appear_in_the_skill_item() {
    let item = compose_plugin_skill(&catalog("opaque-plugin"));
    for verb in [VERB_A, VERB_B] {
        assert!(
            item.body
                .contains(&format!("codeterm plugin opaque-plugin {verb}")),
            "{verb} is reachable from the item"
        );
    }
    assert_eq!(item.title, plugin_skill_title("opaque-plugin"));
    assert_eq!(item.source, plugin_skill_source("opaque-plugin"));
    assert!(item.source.starts_with(PLUGIN_SKILL_SOURCE_PREFIX));
}

#[test]
fn declared_keywords_and_verb_names_become_tags() {
    let item = compose_plugin_skill(&catalog("opaque-plugin"));
    for tag in [
        "kw-one",
        "kw-two",
        "zz-alpha",
        "zz-beta",
        PLUGIN_SKILL_TAG,
        "opaque-plugin",
    ] {
        assert!(item.tags.iter().any(|t| t == tag), "missing tag {tag}");
    }
    let unique: std::collections::HashSet<_> = item.tags.iter().collect();
    assert_eq!(unique.len(), item.tags.len());
}

#[test]
fn keywords_fall_back_to_name_and_description_words() {
    let c = AgentVerbCatalog::compose(
        "p",
        "Quokka Relay",
        Some("Moves wombat parcels"),
        &strings(&[VERB_A]),
        None,
        &[],
    );
    for word in ["quokka", "relay", "moves", "wombat", "parcels", "zz-alpha"] {
        assert!(c.keywords.iter().any(|k| k == word), "missing {word}");
    }
}

#[test]
fn verbs_fall_back_to_the_config_help_verb_sentence() {
    let help = format!(
        "Setup uses `not-a-verb`. Agent verbs: `{VERB_A}`, `{VERB_B}` (note). Later `ignored-span`."
    );
    let c = AgentVerbCatalog::compose("p", "P", None, &[], Some(&help), &[]);
    assert_eq!(c.verbs, strings(&[VERB_A, VERB_B]));
}

#[test]
fn declared_verbs_win_over_config_help() {
    let help = "Agent verbs: `from-help`.";
    let c = AgentVerbCatalog::compose("p", "P", None, &strings(&[VERB_A]), Some(help), &[]);
    assert_eq!(c.verbs, strings(&[VERB_A]));
}

#[test]
fn config_help_without_the_marker_yields_no_verbs() {
    assert!(verbs_from_config_help("Run `codeterm plugin p health`.").is_empty());
}

#[test]
fn install_plans_an_upsert_and_a_repeat_pass_plans_nothing() {
    let catalogs = [catalog("opaque-plugin")];
    let first = plan_plugin_skill_sync(&catalogs, &[]);
    assert_eq!(first.upserts.len(), 1);
    assert!(first.removals.is_empty());

    let existing = [stored(&first.upserts[0], "skill_plugin_opaque_plugin")];
    assert_eq!(
        plan_plugin_skill_sync(&catalogs, &existing),
        PluginSkillPlan::default()
    );
}

#[test]
fn uninstall_plans_the_removal_of_its_item() {
    let item = compose_plugin_skill(&catalog("gone-plugin"));
    let kept = compose_plugin_skill(&catalog("kept-plugin"));
    let existing = [stored(&item, "mem-gone"), stored(&kept, "mem-kept")];
    let plan = plan_plugin_skill_sync(&[catalog("kept-plugin")], &existing);
    assert_eq!(plan.removals, vec!["mem-gone".to_string()]);
    assert!(plan.upserts.is_empty());
}

#[test]
fn a_changed_manifest_replaces_the_item() {
    let old = compose_plugin_skill(&catalog("opaque-plugin"));
    let changed = AgentVerbCatalog::compose(
        "opaque-plugin",
        "Opaque Name",
        None,
        &strings(&["zz-gamma"]),
        None,
        &[],
    );
    let plan = plan_plugin_skill_sync(&[changed], &[stored(&old, "mem-1")]);
    assert_eq!(plan.upserts.len(), 1);
    assert!(plan.upserts[0]
        .body
        .contains("codeterm plugin opaque-plugin zz-gamma"));
    assert!(plan.removals.is_empty());
}

const GUIDE_TOKEN: &str = "guide-token-q7";

#[test]
fn a_catalog_without_a_guide_composes_the_unguided_body() {
    let plain = compose_plugin_skill(&catalog("opaque-plugin"));
    for absent in [None, Some("  \n".to_string())] {
        let item = compose_plugin_skill(&catalog("opaque-plugin").with_guide(absent));
        assert_eq!(item, plain);
    }
    assert!(!plain.body.contains(AGENT_GUIDE_HEADING));
}

#[test]
fn a_guide_is_appended_after_the_unguided_body() {
    let plain = compose_plugin_skill(&catalog("opaque-plugin"));
    let guided = compose_plugin_skill(
        &catalog("opaque-plugin").with_guide(Some(format!("\n{GUIDE_TOKEN}\n"))),
    );
    assert!(guided.body.starts_with(&plain.body));
    let tail = &guided.body[plain.body.len()..];
    assert_eq!(tail, format!("{AGENT_GUIDE_HEADING}{GUIDE_TOKEN}\n"));
    assert_eq!(guided.title, plain.title);
    assert_eq!(guided.source, plain.source);
}

#[test]
fn a_changed_guide_replaces_the_item_and_an_unchanged_one_does_not() {
    let unguided = compose_plugin_skill(&catalog("opaque-plugin"));
    let guided = [catalog("opaque-plugin").with_guide(Some(GUIDE_TOKEN.to_string()))];
    let plan = plan_plugin_skill_sync(&guided, &[stored(&unguided, "mem-1")]);
    assert_eq!(plan.upserts.len(), 1);
    assert!(plan.upserts[0].body.contains(GUIDE_TOKEN));

    let current = stored(&plan.upserts[0], "mem-1");
    assert_eq!(
        plan_plugin_skill_sync(&guided, &[current]),
        PluginSkillPlan::default()
    );
}
