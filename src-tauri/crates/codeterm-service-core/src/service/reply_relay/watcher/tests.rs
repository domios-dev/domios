use super::*;
use notify::{Event, EventKind};

#[test]
fn watches_exact_history_and_sidecar_without_unrelated_churn() {
    let history = Path::new("/work/own.history");
    let sidecar = Path::new("/work/own.history.domios-turns.jsonl");
    let sources = watched_sources(history).unwrap();
    assert_eq!(sources, [history.to_path_buf(), sidecar.to_path_buf()]);
    let relevant = |event: &Event| {
        sources
            .iter()
            .any(|source| file_events::affects_source(event, source))
    };
    for path in [history, sidecar] {
        assert!(relevant(
            &Event::new(EventKind::Create(notify::event::CreateKind::File)).add_path(path.into())
        ));
    }
    assert!(!relevant(
        &Event::new(EventKind::Create(notify::event::CreateKind::File))
            .add_path("/work/other.history".into())
    ));
    assert!(!relevant(
        &Event::new(EventKind::Access(notify::event::AccessKind::Any)).add_path(history.into())
    ));
}
