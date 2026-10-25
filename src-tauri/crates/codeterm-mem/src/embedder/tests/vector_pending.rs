use std::sync::Arc;

use crate::embedder::testing::{fixture, note, vector_rows, MockProvider, StepClock, DEADLINE};
use crate::embedder::EmbedderEvent;
use crate::SearchQuery;

#[test]
fn an_outage_queues_vector_pending_and_recovery_backfills_it() {
    let provider = MockProvider::new("mock", "m-8", 8);
    let fx = fixture(provider.clone(), StepClock::new());
    let (sender, recovered) = std::sync::mpsc::channel();
    let sender = parking_lot::Mutex::new(sender);
    fx.embedder.subscribe(Arc::new(move |event| {
        if matches!(event, EmbedderEvent::Recovered) {
            let _ = sender.lock().send(());
        }
    }));

    provider.set_down(true);
    let id = fx
        .mem
        .save(note("Outage note", "saved while the embedder was down"))
        .unwrap();
    assert!(fx.mem.embed_queue.wait_idle(DEADLINE));
    let query = SearchQuery {
        query: "embedder down".into(),
        ..SearchQuery::default()
    };
    assert_eq!(fx.mem.search(&query).unwrap()[0].item.id, id);
    let status = fx.mem.embedder_status();
    assert_eq!((status.pending, status.available), (1, false));
    assert!(vector_rows(&fx.mem).is_empty());

    provider.set_down(false);
    assert_eq!(fx.mem.recall(&query).unwrap()[0].item.id, id);
    recovered.recv_timeout(DEADLINE).unwrap();
    assert!(fx.mem.embed_queue.wait_idle(DEADLINE));
    assert_eq!(vector_rows(&fx.mem), vec![id]);
    assert_eq!(fx.mem.embedder_status().pending, 0);
}
