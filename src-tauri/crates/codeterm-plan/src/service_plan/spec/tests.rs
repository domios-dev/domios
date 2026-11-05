use super::*;
use crate::plan::{OrchestratorRef, Plan};

fn store_root() -> (std::path::PathBuf, PlanStore) {
    let root = tempfile::tempdir().unwrap().keep();
    (root.clone(), PlanStore::new_for_test(&root))
}

fn create_plan(store: &PlanStore, root: &std::path::Path, title: &str) -> Plan {
    store
        .create_plan(
            title.into(),
            root.display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap()
}

fn export_spec_sidecar(store: &PlanStore, root: &std::path::Path, spec_id: &str) {
    let spec = store.get_spec(spec_id).unwrap();
    let global = root.join(".domios/specs").join(format!("{}.yaml", spec.id));
    if let Some(plan_id) = spec.plan_id.as_deref() {
        let plan: Plan =
            serde_json::from_value(store.export_plan_document(plan_id).unwrap()).unwrap();
        let directory = crate::service_plan::store_v2::plan_state_dir(&plan).unwrap();
        let target = directory.join("spec.yaml");
        crate::service_plan::spec_store::write_spec_at(&target, &spec).unwrap();
        let _ = std::fs::remove_file(global);
        for bucket in ["active", "archive"] {
            let previous = root
                .join(".domios/plans")
                .join(bucket)
                .join(plan_id)
                .join("spec.yaml");
            if bucket != crate::service_plan::paths::plan_bucket(&plan.status) {
                let _ = std::fs::remove_file(previous);
            }
        }
    } else {
        crate::service_plan::spec_store::write_spec_at(&global, &spec).unwrap();
    }
}

#[test]
fn orphan_spec_persists_inline_yaml_and_reloads() {
    let (root, store) = store_root();
    let spec = store
        .create_spec("Requirements".into(), root.display().to_string(), 10)
        .unwrap();
    let sid = store
        .add_spec_section(&spec.id, "Login", "body text", None, &[], None, None, 11)
        .unwrap();

    export_spec_sidecar(&store, &root, &spec.id);
    // Orphan (no owning plan) -> global yaml, bodies inline, no json/md fan-out.
    let yaml_path = root
        .join(".domios")
        .join("specs")
        .join(format!("{}.yaml", spec.id));
    assert!(yaml_path.is_file());
    assert!(!root.join(".domios/specs").join(&spec.id).exists());
    assert!(std::fs::read_to_string(&yaml_path)
        .unwrap()
        .contains("body text"));

    let reloaded = PlanStore::new_for_test(&root);
    reloaded.scan_project(&root).unwrap();
    let spec = reloaded.get_spec(&spec.id).unwrap();
    assert_eq!(spec.sections[0].id, sid);
    assert_eq!(spec.sections[0].body, "body text");
    assert_eq!(spec.plan_id, None);
}

#[test]
fn linking_adopts_spec_into_plan_dir() {
    let (root, store) = store_root();
    let spec = store
        .create_spec("Reqs".into(), root.display().to_string(), 1)
        .unwrap();
    let ssec = store
        .add_spec_section(&spec.id, "Req", "", None, &[], None, None, 2)
        .unwrap();
    let plan = create_plan(&store, &root, "Plan");
    let psec = store
        .add_section(&plan.id, "Build", "", None, &[], None, None, 3)
        .unwrap();
    let global = root.join(".domios/specs").join(format!("{}.yaml", spec.id));
    assert!(global.is_file());

    store
        .link_spec_section(&plan.id, &spec.id, &ssec, &psec, 4)
        .unwrap();

    export_spec_sidecar(&store, &root, &spec.id);
    let co_located = crate::service_plan::store_v2::plan_state_dir(&store.get(&plan.id).unwrap())
        .unwrap()
        .join("spec.yaml");
    assert!(co_located.is_file(), "spec.yaml must land in the plan dir");
    assert!(
        !global.exists(),
        "orphan global yaml must be dropped on adoption"
    );
    assert_eq!(
        store.get_spec(&spec.id).unwrap().plan_id.as_deref(),
        Some(plan.id.as_str())
    );
}

#[test]
fn spec_coverage_reports_unplanned_and_two_hop_done() {
    let (root, store) = store_root();
    let spec = store
        .create_spec("Requirements".into(), root.display().to_string(), 10)
        .unwrap();
    let linked = store
        .add_spec_section(&spec.id, "Linked", "", None, &[], None, None, 11)
        .unwrap();
    store
        .add_spec_section(&spec.id, "Unplanned", "", None, &[], None, None, 12)
        .unwrap();
    let plan = create_plan(&store, &root, "Plan");
    let plan_sec = store
        .add_section(&plan.id, "Build", "", None, &[], None, None, 13)
        .unwrap();
    let task = store
        .add_task(
            &plan.id,
            crate::service_plan::NewTask {
                name: "Do it".into(),
                kind: "implement".into(),
                section_ids: vec![plan_sec.clone()],
                competition: None,
                ..Default::default()
            },
            14,
        )
        .unwrap();
    store
        .update_task(
            &plan.id,
            &task,
            None,
            None,
            None,
            None,
            Some("done"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
            None,
            None,
            15,
        )
        .unwrap();
    store
        .link_spec_section(&plan.id, &spec.id, &linked, &plan_sec, 16)
        .unwrap();

    let report = store.spec_coverage_report(&spec.id).unwrap().unwrap();
    assert_eq!(report.unplanned, vec!["Unplanned".to_string()]);
    assert_eq!(report.sections[0].status, "done");
    assert_eq!(report.sections[1].status, "unplanned");
}

#[test]
fn batch_spec_ops_resolve_refs_and_link_grid() {
    let (root, store) = store_root();
    let plan = create_plan(&store, &root, "Plan");
    let plan_sec = store
        .add_section(&plan.id, "Build", "", None, &[], None, None, 2)
        .unwrap();
    let result = store
        .apply_batch(
            &plan.id,
            crate::service_plan::batch::BatchRequest {
                key: None,
                ops: vec![
                    crate::service_plan::batch::BatchOp::UpsertSpec {
                        client_id: Some("sp".into()),
                        spec_id: None,
                        title: "Spec".into(),
                    },
                    crate::service_plan::batch::BatchOp::UpsertSpecSection {
                        client_id: Some("ss".into()),
                        spec: "$sp".into(),
                        section_id: None,
                        title: "Req".into(),
                        body: Some("body".into()),
                    },
                    crate::service_plan::batch::BatchOp::LinkSpecSection {
                        spec: "$sp".into(),
                        spec_section: "$ss".into(),
                        plan_section: plan_sec.clone(),
                    },
                ],
            },
            3,
        )
        .unwrap();
    let spec_id = result.ids.get("sp").unwrap();
    let plan = store.get(&plan.id).unwrap();
    assert_eq!(plan.spec_refs, vec![spec_id.clone()]);
    assert_eq!(plan.spec_links.len(), 1);
}

#[test]
fn archive_carries_co_located_spec_yaml() {
    let (root, store) = store_root();
    let spec = store
        .create_spec("Reqs".into(), root.display().to_string(), 1)
        .unwrap();
    store
        .add_spec_section(&spec.id, "Req", "req body", None, &[], None, None, 2)
        .unwrap();
    let plan = store
        .create_plan_from_spec(
            &spec.id,
            "Plan".into(),
            OrchestratorRef::default(),
            vec![],
            3,
        )
        .unwrap();
    export_spec_sidecar(&store, &root, &spec.id);
    let active_spec = root
        .join(".domios/plans/active")
        .join(&plan.id)
        .join("spec.yaml");
    assert!(active_spec.is_file());

    store.archive(&plan.id, 4).unwrap();
    export_spec_sidecar(&store, &root, &spec.id);

    let archived_spec = root
        .join(".domios/plans/archive")
        .join(&plan.id)
        .join("spec.yaml");
    assert!(
        archived_spec.is_file(),
        "spec.yaml must move with its plan on archive"
    );
    assert!(!active_spec.exists());
    let text = std::fs::read_to_string(&archived_spec).unwrap();
    assert!(text.contains("req body"));
}

#[test]
fn plan_delete_evicts_owned_spec_from_memory() {
    let (root, store) = store_root();
    let spec = store
        .create_spec("Reqs".into(), root.display().to_string(), 1)
        .unwrap();
    store
        .add_spec_section(&spec.id, "Req", "body", None, &[], None, None, 2)
        .unwrap();
    let plan = store
        .create_plan_from_spec(
            &spec.id,
            "Plan".into(),
            OrchestratorRef::default(),
            vec![],
            3,
        )
        .unwrap();
    assert_eq!(
        store.get_spec(&spec.id).unwrap().plan_id.as_deref(),
        Some(plan.id.as_str())
    );

    store.delete(&plan.id).unwrap();

    assert!(
        store.get_spec(&spec.id).is_none(),
        "owned spec must die with its plan"
    );
    assert!(!store.list_spec_index().iter().any(|e| e.spec_id == spec.id));
}

#[test]
fn delete_spec_removes_orphan_file_and_memory() {
    let (root, store) = store_root();
    let spec = store
        .create_spec("Loose".into(), root.display().to_string(), 1)
        .unwrap();
    let yaml = root.join(".domios/specs").join(format!("{}.yaml", spec.id));
    assert!(yaml.is_file());

    store.delete_spec(&spec.id, 2).unwrap();

    assert!(!yaml.exists());
    assert!(store.get_spec(&spec.id).is_none());
}

#[test]
fn delete_spec_scrubs_plan_references() {
    let (root, store) = store_root();
    let spec = store
        .create_spec("Reqs".into(), root.display().to_string(), 1)
        .unwrap();
    let ssec = store
        .add_spec_section(&spec.id, "Req", "", None, &[], None, None, 2)
        .unwrap();
    let plan = create_plan(&store, &root, "Plan");
    let psec = store
        .add_section(&plan.id, "Build", "", None, &[], None, None, 3)
        .unwrap();
    store
        .link_spec_section(&plan.id, &spec.id, &ssec, &psec, 4)
        .unwrap();

    store.delete_spec(&spec.id, 5).unwrap();

    let plan = store.get(&plan.id).unwrap();
    assert!(plan.spec_refs.is_empty());
    assert!(plan.spec_links.is_empty());
    assert_eq!(plan.primary_spec_id, None);
    let co_located = crate::service_plan::store_v2::plan_state_dir(&plan)
        .unwrap()
        .join("spec.yaml");
    assert!(!co_located.exists(), "co-located yaml must be removed");
}

#[test]
fn from_spec_conflict_leaves_no_orphan_plan() {
    let (root, store) = store_root();
    let spec = store
        .create_spec("SCRATCH-dx-spec".into(), root.display().to_string(), 1)
        .unwrap();
    let owner = store
        .create_plan_from_spec(
            &spec.id,
            "SCRATCH-owner-plan".into(),
            OrchestratorRef::default(),
            vec![],
            2,
        )
        .unwrap();
    assert_eq!(store.list_index().unwrap().len(), 1);

    let conflict = store.create_plan_from_spec(
        &spec.id,
        "SCRATCH-conflict-plan".into(),
        OrchestratorRef::default(),
        vec![],
        3,
    );
    assert!(conflict.is_err(), "second from-spec must be rejected");
    assert_eq!(
        store.list_index().unwrap().len(),
        1,
        "spec-conflict must leave zero orphan plans"
    );
    assert_eq!(
        store.get_spec(&spec.id).unwrap().plan_id.as_deref(),
        Some(owner.id.as_str())
    );
}

#[test]
fn effort_lineage_survives_archive() {
    let (root, store) = store_root();
    let a = create_plan(&store, &root, "A");
    let b = create_plan(&store, &root, "B");
    store
        .link_effort(&a.id, &b.id, EffortLinkRel::Follows, 2)
        .unwrap();
    store.archive(&a.id, 3).unwrap();
    let archived = store.get(&a.id).unwrap();
    assert_eq!(archived.status, crate::plan::PlanStatus::Archived);
    assert_eq!(archived.lineage[0].to, b.id);
}
