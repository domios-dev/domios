use super::*;

#[test]
fn resolve_model_matches_exact_id() {
    let id = "test-resolve-exact-id";
    register(
        id,
        RegisteredDefaults {
            models: vec![
                ModelInfo {
                    id: "gpt-5.5".into(),
                    display_name: "GPT-5.5".into(),
                    description: None,
                    available: None,
                    availability: None,
                    featured: None,
                    badge: None,
                    group: None,
                    reasoning_efforts: None,
                },
                ModelInfo {
                    id: "gpt-5.5-mini".into(),
                    display_name: "GPT-5.5 Mini".into(),
                    description: None,
                    available: None,
                    availability: None,
                    featured: None,
                    badge: None,
                    group: None,
                    reasoning_efforts: None,
                },
            ],
            default: Some("gpt-5.5-mini".into()),
        },
    );

    let resolved = resolve_model(id, Some("gpt-5.5"));

    assert_eq!(resolved.id, "gpt-5.5");
    assert_eq!(resolved.display, "GPT-5.5");
    assert!(!resolved.fell_back);
    registry().write().remove(id);
}

#[test]
fn resolve_model_matches_display_case_insensitively() {
    let id = "test-resolve-display-case";
    register(
        id,
        RegisteredDefaults {
            models: vec![ModelInfo {
                id: "gpt-5.5".into(),
                display_name: "GPT-5.5".into(),
                description: None,
                available: None,
                availability: None,
                featured: None,
                badge: None,
                group: None,
                reasoning_efforts: None,
            }],
            default: Some("gpt-5.5".into()),
        },
    );

    let upper = resolve_model(id, Some("GPT-5.5"));
    let lower = resolve_model(id, Some("gpt-5.5"));

    assert_eq!(upper.id, "gpt-5.5");
    assert_eq!(lower.id, "gpt-5.5");
    assert!(!upper.fell_back);
    assert!(!lower.fell_back);
    registry().write().remove(id);
}

#[test]
fn resolve_model_unknown_falls_back_to_default() {
    let id = "test-resolve-unknown-default";
    register(
        id,
        RegisteredDefaults {
            models: vec![
                ModelInfo {
                    id: "gpt-5.5".into(),
                    display_name: "GPT-5.5".into(),
                    description: None,
                    available: None,
                    availability: None,
                    featured: None,
                    badge: None,
                    group: None,
                    reasoning_efforts: None,
                },
                ModelInfo {
                    id: "gpt-5.5-mini".into(),
                    display_name: "GPT-5.5 Mini".into(),
                    description: None,
                    available: None,
                    availability: None,
                    featured: None,
                    badge: None,
                    group: None,
                    reasoning_efforts: None,
                },
            ],
            default: Some("gpt-5.5-mini".into()),
        },
    );

    let resolved = resolve_model(id, Some("stale-model"));

    assert_eq!(resolved.id, "gpt-5.5-mini");
    assert_eq!(resolved.display, "GPT-5.5 Mini");
    assert!(resolved.fell_back);
    registry().write().remove(id);
}
