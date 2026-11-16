use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};

#[tokio::test]
async fn extraction_dreamer_and_calibration_tabs_are_system_authors() {
    for intent in ["extract", "dreaming:rem", "calibration"] {
        let (mut mux, tab) = MultiplexerState::new();
        mux.get_tab_mut(&tab).unwrap().worker_id = Some("worker-id".into());
        let fixture = ServiceFixture::new(mux);
        fixture.service.workers.register(serde_json::from_value(serde_json::json!({
            "worker_id":"worker-id", "tab_id":tab, "intent":intent, "spawned_by":"system", "spawned_at":0,
            "status":"writing", "touched_nodes":[], "color":"test"
        })).unwrap());
        assert_eq!(
            mem_author_for_caller(&fixture.service, Some(&tab))
                .await
                .unwrap(),
            MemAuthor::system(intent)
        );
    }
}
