use super::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[test]
fn repeated_status_and_delivery_cache_missing_key_once() {
    let calls = Arc::new(AtomicUsize::new(0));
    let reads = calls.clone();
    let reader: SecretReader = Arc::new(move |_| {
        reads.fetch_add(1, Ordering::SeqCst);
        Err("missing".into())
    });
    let config = Config {
        mode: crate::Mode::Amplitude,
        ..Config::default()
    };
    let dir = tempfile::tempdir().unwrap();
    let mut engine = crate::engine::Engine::open(dir.path().join("settings.json"), config).unwrap();
    engine.sink = Box::new(Local::with_build_key(config, reader, None));
    engine.consent(crate::Consent::On).unwrap();
    engine.enqueue(crate::Event::PlanViewed {}, 0).unwrap();
    for _ in 0..3 {
        assert!(!engine.status().vendor_available);
        assert!(engine.deliver(0).is_err());
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn repeated_availability_caches_secret_override_with_build_key() {
    let calls = Arc::new(AtomicUsize::new(0));
    let reads = calls.clone();
    let reader: SecretReader = Arc::new(move |_| {
        reads.fetch_add(1, Ordering::SeqCst);
        Ok("override".into())
    });
    let sink = Local::with_build_key(Config::default(), reader, Some("public-key"));
    assert!(sink.available());
    assert!(sink.available());
    assert_eq!(sink.key().map(String::as_str), Some("override"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
