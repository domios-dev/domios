use super::*;

#[test]
fn prior_catalog_schema_cache_is_rejected_before_discovery() {
    let id = "test-prior-catalog-schema";
    cleanup(id);
    let seed = RegisteredDefaults {
        models: vec![model("seed", "Seed")],
        default: Some("seed".into()),
    };
    register(id, seed.clone());
    put_live(id, vec![model("live", "Live")], Some("live".into()));

    let mut previous_schema = serde_json::to_value(read_cache(id).expect("cache written"))
        .expect("cache serializes");
    previous_schema
        .as_object_mut()
        .expect("cache object")
        .remove("schema_version");
    previous_schema["models"][0]
        .as_object_mut()
        .expect("cached model object")
        .remove("default_reasoning_effort");
    std::fs::write(cache_path(id), previous_schema.to_string()).unwrap();

    registry().write().remove(id);
    seed_registry().write().remove(id);
    register(id, seed);
    prime_from_cache(id);

    assert_eq!(resolve_models(id)[0].id, "seed");
    assert!(is_stale(id, DEFAULT_TTL, SystemTime::now()));
    cleanup(id);
}
