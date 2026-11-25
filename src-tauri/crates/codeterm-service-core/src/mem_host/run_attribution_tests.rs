use super::with_worker_write;
use crate::{
    mem::workers::make_info, multiplexer::state::MultiplexerState,
    service::test_fixture::ServiceFixture,
};

#[test]
fn run_attribution_uses_lifecycle_owned_worker_and_restores_nested_origin() {
    let (mux, tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let svc = &fixture.service;
    let mut worker = make_info(
        "gw_owner".into(),
        tab.clone(),
        "dreaming:rem".into(),
        "test".into(),
        "blue".into(),
    );
    svc.workers.register(worker.clone());
    svc.mux.write().get_tab_mut(&tab).unwrap().worker_id = Some("gw_owner".into());
    with_worker_write(svc, Some(&tab), || {
        assert_eq!(
            crate::mem::store::current_write_worker().as_deref(),
            Some("gw_owner")
        );
        with_worker_write(svc, None, || {
            assert_eq!(crate::mem::store::current_write_worker(), None)
        });
        assert_eq!(
            crate::mem::store::current_write_worker().as_deref(),
            Some("gw_owner")
        );
    });
    assert_eq!(crate::mem::store::current_write_worker(), None);
    worker.tab_id = "another_tab".into();
    svc.workers.register(worker);
    with_worker_write(svc, Some(&tab), || {
        assert_eq!(
            crate::mem::store::current_write_worker().as_deref(),
            Some("gw_owner")
        )
    });
}

#[test]
fn revoked_worker_writes_stay_refused_after_terminal_and_tab_removal() {
    for remove_tab in [false, true] {
        let (mux, tab) = MultiplexerState::new();
        let fixture = ServiceFixture::new(mux);
        let svc = &fixture.service;
        let new_item = |title: &str| crate::mem::NewMemItem {
            kind: crate::mem::MemKind::Note,
            scope: crate::mem::MemScope::Global,
            title: title.into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "revocation proof".into(),
            source: None,
            importance: 2.0,
            source_path: None,
            pinned: false,
        };
        let source = svc.mem.save(new_item("source")).unwrap();
        let target = svc.mem.save(new_item("target")).unwrap();
        let uid = svc.mem.get(&source).unwrap().unwrap().uid;
        let generation = svc.mem.extraction_authority().open(&source, &uid);
        svc.mem
            .extraction_authority()
            .bind_worker(&generation, "gw_revoked");
        svc.workers.register(make_info(
            "gw_revoked".into(),
            tab.clone(),
            format!("extract:{source}"),
            "test".into(),
            "blue".into(),
        ));
        svc.mux.write().get_tab_mut(&tab).unwrap().worker_id = Some("gw_revoked".into());
        let queued_origin = super::WorkerWriteOrigin::capture(svc, Some(&tab));
        assert!(svc.mem.delete(&source).unwrap());
        svc.workers.kill("gw_revoked").unwrap();
        if remove_tab {
            svc.mux.write().remove_tab(&tab).unwrap();
        }
        let update = || {
            svc.mem.update(
                &target,
                crate::mem::store::MemUpdate {
                    importance: Some(4.0),
                    ..Default::default()
                },
            )
        };
        assert!(
            queued_origin.run(svc, update).is_err(),
            "queued write, removed={remove_tab}"
        );
        assert!(
            with_worker_write(svc, Some(&tab), update).is_err(),
            "late write, removed={remove_tab}"
        );
        assert_eq!(svc.mem.get(&target).unwrap().unwrap().importance, 2.0);
        assert!(svc
            .workers
            .get("gw_revoked")
            .unwrap()
            .touched_nodes
            .is_empty());
        assert_eq!(crate::mem::store::current_write_worker(), None);
    }
}
