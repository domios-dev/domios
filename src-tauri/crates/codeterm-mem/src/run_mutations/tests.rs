use crate::curation::merge::MergeRequest;
use crate::{
    backend::contract::item,
    store::{with_write_worker, MemUpdate},
    workers::{make_info, CreatedEdge, GraphWorkerRegistry, NodeAction},
    MemManager, Relation,
};

fn worker(registry: &GraphWorkerRegistry, id: &str) {
    registry.register(make_info(
        id.into(),
        format!("tab_{id}"),
        "dreaming:rem".into(),
        "test".into(),
        "blue".into(),
    ));
}

fn edge(from: &str, to: &str) -> Relation {
    Relation {
        from_id: from.into(),
        to_id: to.into(),
        relation_type: "about".into(),
        scope: "global".into(),
        valid_from: "2026-11-24T05:26:54Z".into(),
        valid_to: None,
        confidence: 1.0,
        properties: serde_json::json!({}),
        reason: None,
    }
}

#[test]
fn committed_mutations_are_attributed_to_exact_run_and_survive_history_reload() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("workers.json");
    let registry = GraphWorkerRegistry::load(path.clone());
    worker(&registry, "gw_one");
    worker(&registry, "gw_two");
    let mem = MemManager::open_in_memory().unwrap();
    mem.attach_worker_registry(&registry);
    let keeper = mem.save(item("keeper")).unwrap();
    let lesser = mem.save(item("lesser")).unwrap();
    let archived = mem.save(item("archived")).unwrap();
    let reweighted = mem.save(item("reweighted")).unwrap();
    with_write_worker(Some("gw_one"), || {
        mem.graph_relate(&edge(&keeper, &lesser)).unwrap();
        mem.merge_items(
            &MergeRequest {
                keeper: &keeper,
                lesser: &lesser,
                body: None,
                conflict: false,
            },
            "test",
            10,
            chrono::Utc::now(),
        )
        .unwrap();
        mem.archive(&archived, "test", "test").unwrap();
        mem.update(
            &reweighted,
            MemUpdate {
                importance: Some(4.0),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(mem.archive("missing", "test", "test").is_err());
    });
    with_write_worker(Some("gw_two"), || {
        mem.graph_relate(&edge(&reweighted, &keeper)).unwrap()
    });
    mem.graph_relate(&edge("outside_run", "outside_anchor"))
        .unwrap();
    registry
        .update_progress("gw_one", "write_diary".into())
        .unwrap();
    registry.complete("gw_one", "committed".into()).unwrap();
    let restored = GraphWorkerRegistry::load(path);
    let one = restored.get("gw_one").unwrap();
    assert_eq!(one.node_actions[&keeper], NodeAction::Merged);
    assert_eq!(one.node_actions[&lesser], NodeAction::Merged);
    assert_eq!(one.node_actions[&archived], NodeAction::Archived);
    assert_eq!(one.node_actions[&reweighted], NodeAction::Reweighted);
    assert!(one
        .node_actions
        .keys()
        .all(|id| one.touched_nodes.contains(id)));
    assert!(!one.touched_nodes.contains("missing"));
    assert!(!one.touched_nodes.contains("outside_run"));
    assert_eq!(
        one.created_edges,
        vec![
            CreatedEdge {
                from: keeper.clone(),
                to: lesser.clone(),
                relation: "about".into()
            },
            CreatedEdge {
                from: lesser.clone(),
                to: keeper.clone(),
                relation: "merged_into".into()
            },
        ]
    );
    assert_eq!(one.stage.as_deref(), Some("write_diary"));
    assert_eq!(
        restored.get("gw_two").unwrap().created_edges,
        vec![CreatedEdge {
            from: reweighted,
            to: keeper,
            relation: "about".into()
        }]
    );
}

#[test]
fn topic_routing_records_actual_edges_and_ignores_failed_operations() {
    let temp = tempfile::tempdir().unwrap();
    let registry = GraphWorkerRegistry::load(temp.path().join("workers.json"));
    worker(&registry, "gw_topic");
    let mem = MemManager::open_in_memory().unwrap();
    mem.attach_worker_registry(&registry);
    let source = mem.save(item("source")).unwrap();
    with_write_worker(Some("gw_topic"), || {
        let result = mem
            .graph_introduce_topic(codeterm_mem_store::graph::IntroduceTopicOptions {
                topic_id: "topic_test".into(),
                entity_type: "Topic".into(),
                sources: vec![source.clone()],
                anchor: "project_test".into(),
                rel_source: "about".into(),
                rel_anchor: "belongs_to".into(),
                reason: None,
            })
            .unwrap();
        assert_eq!(result.edges_created, 2);
        let result = mem.graph_introduce_topic(codeterm_mem_store::graph::IntroduceTopicOptions {
            topic_id: "topic_test".into(),
            entity_type: "Wrong".into(),
            sources: vec!["uncommitted".into()],
            anchor: "wrong_anchor".into(),
            rel_source: "about".into(),
            rel_anchor: "belongs_to".into(),
            reason: None,
        });
        assert!(result.is_err());
    });
    let run = registry.get("gw_topic").unwrap();
    assert_eq!(run.created_edges.len(), 2);
    assert_eq!(run.touched_nodes.len(), 3);
    assert_eq!(run.node_actions[&source], NodeAction::Linked);
    assert!(!run.touched_nodes.contains("uncommitted"));
}

#[test]
fn stage_updates_use_fixed_ids_and_scoped_origin_restores_after_unwind() {
    let temp = tempfile::tempdir().unwrap();
    let registry = GraphWorkerRegistry::load(temp.path().join("workers.json"));
    worker(&registry, "gw_stage");
    for stage in crate::workers::DREAM_STAGES {
        registry.update_progress("gw_stage", stage.into()).unwrap();
        assert_eq!(
            registry.get("gw_stage").unwrap().stage.as_deref(),
            Some(stage)
        );
    }
    with_write_worker(Some("gw_stage"), || {
        let unwound = std::panic::catch_unwind(|| {
            with_write_worker(Some("gw_other"), || panic!("test unwind"))
        });
        assert!(unwound.is_err());
        assert_eq!(
            crate::store::current_write_worker().as_deref(),
            Some("gw_stage")
        );
    });
    assert_eq!(crate::store::current_write_worker(), None);
    registry
        .update_progress("gw_stage", "human progress".into())
        .unwrap();
    assert_eq!(
        registry.get("gw_stage").unwrap().stage.as_deref(),
        Some("write_diary")
    );
    with_write_worker(Some("gw_outer"), || {
        let _ =
            std::panic::catch_unwind(|| with_write_worker(Some("gw_panicked"), || panic!("test")));
        assert_eq!(
            crate::store::current_write_worker().as_deref(),
            Some("gw_outer")
        );
    });
    assert_eq!(crate::store::current_write_worker(), None);
}

#[test]
fn skeleton_save_binds_own_run_before_final_report() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("workers.json");
    let registry = GraphWorkerRegistry::load(path.clone());
    worker(&registry, "gw_diary");
    worker(&registry, "gw_other");
    let mem = MemManager::open_in_memory().unwrap();
    mem.attach_worker_registry(&registry);
    let mut skeleton = item("first skeleton");
    skeleton.kind = crate::MemKind::Daily;
    skeleton.area = Some("_dreams".into());
    skeleton.tags = vec!["dream-diary".into()];
    let id = with_write_worker(Some("gw_diary"), || mem.save(skeleton.clone())).unwrap();
    assert_eq!(
        registry.get("gw_diary").unwrap().diary_id.as_deref(),
        Some(id.as_str())
    );
    assert_eq!(
        registry.live_owner_of_diary(&id).as_deref(),
        Some("gw_diary")
    );
    assert_eq!(
        registry.get("gw_diary").unwrap().finalization,
        crate::dream_status::DiaryFinalization::Pending
    );
    assert!(registry.get("gw_other").unwrap().diary_id.is_none());
    skeleton.title = "second skeleton".into();
    with_write_worker(Some("gw_diary"), || mem.save(skeleton.clone())).unwrap();
    assert_eq!(
        registry.get("gw_diary").unwrap().diary_id.as_deref(),
        Some(id.as_str())
    );
    registry.complete("gw_diary", "finished".into()).unwrap();
    assert!(registry.live_owner_of_diary(&id).is_none());
    let loaded = GraphWorkerRegistry::load(path);
    assert_eq!(
        loaded.get("gw_diary").unwrap().diary_id.as_deref(),
        Some(id.as_str())
    );
}
