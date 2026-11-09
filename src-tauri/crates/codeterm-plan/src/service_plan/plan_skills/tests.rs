use super::*;
use crate::plan::OrchestratorRef;
use std::io::Write;

struct ExternalPlanCleanup(std::path::PathBuf);

#[test]
fn plan_skill_manifest_uses_latest_keep_verdict_and_exact_digest() {
    let temp = tempfile::tempdir().unwrap();
    let skills = temp.path().join("skills");
    for (slug, body) in [
        (
            "alpha",
            "---\ntitle: Alpha\ndescription: First purpose.\n---\n\n# Alpha\n",
        ),
        (
            "beta",
            "---\ntitle: Beta\ndescription: Discarded.\n---\n\n# Beta\n",
        ),
    ] {
        std::fs::create_dir_all(skills.join(slug)).unwrap();
        std::fs::write(skills.join(slug).join("SKILL.md"), body).unwrap();
    }
    std::fs::create_dir_all(skills.join("stray")).unwrap();
    std::fs::write(
            skills.join("ledger.jsonl"),
            "{\"seq\":1,\"slug\":\"alpha\",\"verdict\":\"discard\",\"rationale\":\"old\"}\n{\"seq\":2,\"slug\":\"alpha\",\"verdict\":\"keep\",\"rationale\":\"latest\"}\n{\"seq\":3,\"slug\":\"beta\",\"verdict\":\"superseded\",\"rationale\":\"latest\"}\n",
        )
        .unwrap();
    let manifest = plan_skill_manifest_from_dir(temp.path()).unwrap();
    assert_eq!(manifest.len(), 1);
    assert_eq!(manifest[0].slug, "alpha");
    assert_eq!(manifest[0].title, "Alpha");
    assert_eq!(manifest[0].purpose, "First purpose.");
    assert_eq!(
        manifest[0].content_digest,
        format!(
            "sha256:{:x}",
            Sha256::digest(std::fs::read(skills.join("alpha/SKILL.md")).unwrap())
        )
    );
}

impl Drop for ExternalPlanCleanup {
    fn drop(&mut self) {
        if self.0.is_dir() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

fn database_fingerprint() -> Option<(u64, std::time::SystemTime)> {
    let metadata =
        std::fs::metadata(crate::service_plan::paths::global_plan_database_path()).ok()?;
    Some((metadata.len(), metadata.modified().ok()?))
}

fn external_plan_dir(plan: &crate::plan::Plan) -> std::path::PathBuf {
    crate::service_plan::paths::plan_state_dir_unchecked(
        crate::service_plan::paths::plan_sidecar_root(plan),
        &plan.status,
        &plan.id,
    )
}

fn write_skill(plan_dir: &Path, slug: &str, md: &str) {
    let dir = plan_dir.join("skills").join(slug);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("SKILL.md"), md).unwrap();
}

#[test]
fn parse_frontmatter_extracts_title_check_and_purpose() {
    let md = "---\ntitle: Deploy\ncheck: ./scripts/deploy.sh\n---\n\n_Ship it fast._\n\n## Steps\n";
    let (title, purpose, check) = parse_frontmatter(md);
    assert_eq!(title.as_deref(), Some("Deploy"));
    assert_eq!(purpose.as_deref(), Some("Ship it fast."));
    assert_eq!(check.as_deref(), Some("./scripts/deploy.sh"));
}

#[test]
fn parse_frontmatter_prefers_persisted_description_metadata() {
    let md = "---\ntitle: Deploy\ndescription: >-\n  Deploy staging safely without replacing production.\n---\n\nFallback body.\n";
    let (_, purpose, _) = parse_frontmatter(md);
    assert_eq!(
        purpose.as_deref(),
        Some("Deploy staging safely without replacing production.")
    );
}

#[test]
fn malformed_or_placeholder_skill_has_no_invented_purpose() {
    assert_eq!(
        persisted_skill_purpose("---\ntitle: Broken\nNo closing boundary"),
        None
    );
    assert_eq!(
            persisted_skill_purpose(
                "---\ntitle: Scaffold\n---\n\n_One-paragraph summary that becomes the search snippet._\n"
            ),
            None
        );
}

#[test]
fn plan_skill_manifest_from_sidecar_matches_list() {
    let tmp = tempfile::tempdir().unwrap();
    crate::service_plan::paths::mark_test_project_root(tmp.path());
    let store = PlanStore::new_for_test(tmp.path());
    let plan = store
        .create_plan(
            "Skills".into(),
            tmp.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
            vec![],
            1,
        )
        .unwrap();
    let _cleanup = ExternalPlanCleanup(external_plan_dir(&plan));
    let dir = crate::service_plan::paths::plan_state_dir(
        crate::service_plan::paths::plan_sidecar_root(&plan),
        &plan.status,
        &plan.id,
    )
    .unwrap();
    write_skill(
        &dir,
        "deploy",
        "---\ntitle: Deploy\ncheck: echo ok\n---\n\nDeploy staging safely.\n",
    );
    write_skill(
        &dir,
        ".deploy.pending",
        "---\ntitle: Pending\n---\n\nNot committed yet.\n",
    );
    std::fs::write(
        dir.join("skills/ledger.jsonl"),
        "{\"seq\":1,\"slug\":\"deploy\",\"verdict\":\"keep\",\"rationale\":\"fixture\"}\n",
    )
    .unwrap();
    let manifest = plan_skill_manifest(&store, &plan.id).unwrap();
    let listed = list_plan_skills(&store, &plan.id).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(manifest.len(), listed.len());
    assert_eq!(listed[0].slug, "deploy");
    assert_eq!(listed[0].title, "Deploy");
    assert!(listed[0].purpose.contains("Deploy staging"));
    assert_eq!(listed[0].check.as_deref(), Some("echo ok"));
    assert_eq!(manifest[0].content_digest, listed[0].content_digest);
    let heartbeat_skills = crate::service_plan::skill_registry::list_plan_skill_summaries(&plan);
    assert_eq!(
        heartbeat_skills,
        [
            crate::service_plan::skill_registry::PlanSkillRegistrySummary {
                slug: "deploy".to_string(),
                purpose: Some("Deploy staging safely.".to_string()),
            }
        ]
    );

    let detail = show_plan_skill(&store, &plan.id, "deploy").unwrap();
    assert!(detail.body.contains("Deploy staging"));
}

#[test]
fn create_skill_scaffolds_and_ledgers_once() {
    let tmp = tempfile::tempdir().unwrap();
    crate::service_plan::paths::mark_test_project_root(tmp.path());
    let store = PlanStore::new_for_test(tmp.path());
    let plan = store
        .create_plan(
            "Skills".into(),
            tmp.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
            vec![],
            1,
        )
        .unwrap();
    let _cleanup = ExternalPlanCleanup(external_plan_dir(&plan));
    let created = create_plan_skill(
        &store,
        &plan.id,
        NewPlanSkill {
            slug: "Deploy Safely".into(),
            title: Some("Safe Deploy".into()),
            area: Some("release".into()),
            check: Some("./scripts/check.sh".into()),
            rationale: Some("accepted".into()),
        },
    )
    .unwrap();
    assert_eq!(created.slug, "deploy-safely");
    assert_eq!(created.title, "Safe Deploy");
    assert_eq!(created.check.as_deref(), Some("./scripts/check.sh"));
    assert_eq!(created.ledger_seq, 1);
    assert_eq!(created.verdict, "keep");

    let detail = show_plan_skill(&store, &plan.id, &created.slug).unwrap();
    assert!(detail.body.contains("area: release"));
    assert!(detail.body.contains("check: ./scripts/check.sh"));
    assert_eq!(detail.ledger.len(), 1);
    assert_eq!(detail.ledger[0].seq, 1);
    assert_eq!(detail.ledger[0].rationale, "accepted");
    assert!(Path::new(&created.skill_md).is_file());
    assert!(Path::new(&created.scripts).is_dir());

    let duplicate = create_plan_skill(
        &store,
        &plan.id,
        NewPlanSkill {
            slug: "Deploy Safely".into(),
            title: None,
            area: None,
            check: None,
            rationale: None,
        },
    )
    .unwrap_err();
    assert!(matches!(
        duplicate,
        PlanError::PlanSkillAlreadyExists { .. }
    ));
    assert_eq!(
        show_plan_skill(&store, &plan.id, &created.slug)
            .unwrap()
            .ledger
            .len(),
        1
    );
}

#[test]
fn create_skill_write_failure_leaves_no_partial_skill_or_ledger() {
    let tmp = tempfile::tempdir().unwrap();
    crate::service_plan::paths::mark_test_project_root(tmp.path());
    let store = PlanStore::new_for_test(tmp.path());
    let plan = store
        .create_plan(
            "Skills".into(),
            tmp.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
            vec![],
            1,
        )
        .unwrap();
    let _cleanup = ExternalPlanCleanup(external_plan_dir(&plan));
    let dir = crate::service_plan::paths::plan_state_dir(
        crate::service_plan::paths::plan_sidecar_root(&plan),
        &plan.status,
        &plan.id,
    )
    .unwrap();
    std::fs::write(dir.join("skills"), "blocked").unwrap();

    let error = create_plan_skill(
        &store,
        &plan.id,
        NewPlanSkill {
            slug: "deploy".into(),
            title: None,
            area: None,
            check: None,
            rationale: None,
        },
    )
    .unwrap_err();
    assert!(matches!(error, PlanError::Io(_)));
    assert!(dir.join("skills").is_file());
    assert!(!dir.join("skills").join("deploy").exists());
    assert!(!dir.join("skills").join("ledger.jsonl").exists());
}

#[test]
fn test_plan_coordination_stays_in_its_isolated_sidecar_root() {
    let project = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(project.path());
    let plan = store
        .create_plan(
            "External coordination".into(),
            project.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
            vec![],
            1,
        )
        .unwrap();
    let plan_dir = external_plan_dir(&plan);
    let _cleanup = ExternalPlanCleanup(plan_dir.clone());
    assert!(plan_dir.join("plan.yaml").is_file());

    let created = create_plan_skill(
        &store,
        &plan.id,
        NewPlanSkill {
            slug: "coordinate".into(),
            title: None,
            area: None,
            check: None,
            rationale: Some("captured from live coordination".into()),
        },
    )
    .unwrap();
    assert_eq!(list_plan_skills(&store, &plan.id).unwrap().len(), 1);
    let detail = show_plan_skill(&store, &plan.id, &created.slug).unwrap();
    assert_eq!(detail.ledger.len(), 1);
    assert_eq!(
        detail.ledger[0].rationale,
        "captured from live coordination"
    );
    assert!(plan_dir.starts_with(project.path().canonicalize().unwrap()));
}

#[test]
fn plan_skills_resolve_against_the_plan_sidecar_root() {
    let database_before = database_fingerprint();
    let project = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(project.path());
    let plan = store
        .create_plan(
            "External skills".into(),
            project.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
            vec![],
            1,
        )
        .unwrap();
    let plan_dir = external_plan_dir(&plan);
    let _cleanup = ExternalPlanCleanup(plan_dir.clone());

    let created = create_plan_skill(
        &store,
        &plan.id,
        NewPlanSkill {
            slug: "external-state".into(),
            title: Some("External State".into()),
            area: Some("planning".into()),
            check: None,
            rationale: Some("initial verdict".into()),
        },
    )
    .unwrap();
    let skill_dir = plan_dir.join("skills").join("external-state");
    let ledger_path = plan_dir.join("skills").join("ledger.jsonl");
    assert_eq!(Path::new(&created.skill_md), skill_dir.join("SKILL.md"));
    assert!(skill_dir.join("SKILL.md").is_file());
    assert!(ledger_path.is_file());

    let mut ledger = std::fs::OpenOptions::new()
        .append(true)
        .open(&ledger_path)
        .unwrap();
    writeln!(
        ledger,
        "{}",
        serde_json::json!({
            "seq": 2,
            "kind": "skill",
            "slug": "external-state",
            "verdict": "revise",
            "rationale": "history remains external",
        })
    )
    .unwrap();
    drop(ledger);

    // The AI-context manifest (heartbeat/prompt injection) is a different, deliberately keep-only view and stays empty here.
    let listed = list_plan_skills(&store, &plan.id).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].slug, "external-state");
    assert_eq!(listed[0].verdict, "revise");
    assert_eq!(
        crate::service_plan::skill_registry::count_plan_skills_for(&plan),
        0
    );
    assert_eq!(
        crate::service_plan::skill_registry::list_plan_skill_slugs(&plan),
        Vec::<String>::new()
    );
    let detail = show_plan_skill(&store, &plan.id, "external-state").unwrap();
    assert_eq!(detail.ledger.len(), 2);
    assert_eq!(detail.ledger[1].rationale, "history remains external");
    assert_eq!(database_fingerprint(), database_before);
}

#[test]
fn no_legacy_repo_local_fallback_remains() {
    let project = tempfile::tempdir().unwrap();
    let database = tempfile::tempdir().unwrap();
    crate::service_plan::search_index::init_search_port(std::sync::Arc::new(
        crate::service_plan::test_support::TestSearchPort,
    ));
    let store = PlanStore::open(database.path().join("plans.db")).unwrap();
    let plan = store
        .create_plan(
            "Ignore legacy state".into(),
            project.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
            vec![],
            1,
        )
        .unwrap();
    let _cleanup = ExternalPlanCleanup(external_plan_dir(&plan));
    let legacy_skill = project
        .path()
        .join(".codeterm")
        .join("plans")
        .join("active")
        .join(&plan.id)
        .join("skills")
        .join("legacy");
    std::fs::create_dir_all(&legacy_skill).unwrap();
    std::fs::write(
        legacy_skill.join("SKILL.md"),
        "---\ntitle: Legacy\n---\n\nMust not be adopted.\n",
    )
    .unwrap();

    assert!(list_plan_skills(&store, &plan.id).unwrap().is_empty());
    assert!(crate::service_plan::skill_registry::list_plan_skill_summaries(&plan).is_empty());
    assert!(matches!(
        show_plan_skill(&store, &plan.id, "legacy"),
        Err(PlanError::NotFound)
    ));
}
