use super::*;
use crate::ports::sentinel::{
    BlueprintRecord, BlueprintStore, SentinelCleanup, SentinelPorts, SentinelWake,
};
use std::sync::Arc;

struct Blueprints(Arc<Mutex<codeterm_sentinel::store::Store>>);
impl BlueprintStore for Blueprints {
    fn records(&self) -> Vec<BlueprintRecord> {
        Vec::new()
    }
    fn scope_of(&self, id: &str) -> Option<(String, Option<String>)> {
        self.0
            .lock()
            .get(id)
            .map(|job| (job.workspace_id.clone(), job.team_id.clone()))
    }
    fn insert_restored(&self, record: &BlueprintRecord) -> Result<(), String> {
        let job = serde_json::from_value(record.definition.clone()).map_err(|e| e.to_string())?;
        codeterm_sentinel::storage::commit(&self.0, |store| store.stage_insert(job)).map(|_| ())
    }
    fn ensure_external_commit_ready(&self) -> Result<(), String> {
        self.0.lock().ensure_external_commit_ready()
    }
    fn adopt_external_commit(
        &self,
        _: &codeterm_storage::Committed<codeterm_storage::SentinelCommit>,
    ) -> Result<(), String> {
        Ok(())
    }
}
impl SentinelWake for Blueprints {
    fn notify(&self) {}
    fn notify_event_subscription_changed(&self, _: &str) {}
    fn terminal_output(&self, _: &str, _: Option<u64>, _: &[u8]) {}
}
impl SentinelCleanup for Blueprints {
    fn cancel_tab_owned_runs(&self, _: &str) -> usize {
        0
    }
    fn remove_worker_watchers(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn remove_team(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn remove_workspace(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn blueprint_v2_check_restore_closed_workspace_with_v1_receipt_and_no_partial_refusal() {
    for corrupt in [false, true] {
        let mut mux = crate::multiplexer::state::MultiplexerState::empty();
        let workspace = mux.create_workspace("Closed fixture".into(), None).id;
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        let store = Arc::new(Mutex::new(codeterm_sentinel::store::Store::new(
            fixture.temp.path().join("blueprints.json"),
            crate::cron::clock::MockClock::starting_at(1000),
        )));
        let adapter = Arc::new(Blueprints(store.clone()));
        assert!(fixture
            .service
            .set_ext(SentinelPorts {
                store: adapter.clone(),
                wake: adapter.clone(),
                cleanup: adapter
            })
            .is_ok());
        let mut native = native_saved_blueprint();
        native["workspace_id"] = serde_json::json!(workspace);
        let mut obsolete = native.clone();
        obsolete.as_object_mut().unwrap().remove("document");
        obsolete["id"] = serde_json::json!("obsolete");
        if corrupt {
            native["document"]["steps"][0]["routes"] = serde_json::json!({"done":"read"});
        }
        let mut record = fixture
            .service
            .mux
            .read()
            .capture_structure(&workspace, None, "close-fixture".into(), 1000)
            .unwrap();
        record.resources = serde_json::to_value(Resources {
            blueprints: vec![obsolete, native],
            ..Default::default()
        })
        .unwrap();
        record.state = StructureState::Closed;
        fixture
            .service
            .mux
            .write()
            .close_workspace(&workspace)
            .unwrap();
        fixture
            .service
            .mux
            .read()
            .save_closed_structure(&record)
            .unwrap();
        let storage = fixture.install_seeded_storage_desk();
        let desk = crate::session::store::StorageDeskAdapter::from_storage(storage);
        assert!(!desk
            .snapshot()
            .unwrap()
            .workspaces
            .iter()
            .any(|saved| saved.id == workspace));
        let result = fixture.service.restore_close_inventory(&record.id);
        let saved = fixture
            .service
            .mux
            .read()
            .closed_structure(&record.id)
            .unwrap()
            .unwrap();
        let persisted_workspace = desk
            .snapshot()
            .unwrap()
            .workspaces
            .into_iter()
            .find(|saved| saved.id == workspace);
        if corrupt {
            assert!(result.is_err());
            assert!(persisted_workspace.is_none());
            assert!(fixture
                .service
                .mux
                .read()
                .get_workspace(&workspace)
                .is_none());
            assert!(store.lock().jobs.is_empty());
            assert_eq!(saved.state, StructureState::Closed);
            assert_eq!(saved.resources, record.resources);
        } else {
            assert!(result.unwrap().is_some());
            assert!(persisted_workspace.is_some());
            assert!(fixture
                .service
                .mux
                .read()
                .get_workspace(&workspace)
                .is_some());
            assert_eq!(store.lock().jobs.len(), 1);
            assert_eq!(saved.state, StructureState::Restored);
            assert_eq!(
                saved.resources["discarded_v1_blueprints"][0]["id"],
                "obsolete"
            );
            fixture.service.restore_close_inventory(&record.id).unwrap();
            assert_eq!(store.lock().jobs.len(), 1);
        }
        crate::service::session_persist::stop_worker(&fixture.service).unwrap();
    }
}
