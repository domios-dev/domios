//! Unit tests for the parent `model_catalog` module. Split out to keep
//! `model_catalog.rs` under the 1000-line source limit (AGENTS.md).

use super::*;

// The registry is a process-global static — tests run in parallel against it. Use a unique provider id per test (and clean up after) so no
// test depends on the registry being empty or seeing another test's writes.

#[test]
fn register_and_read_back() {
    let id = "test-register-read-back";
    register(
        id,
        RegisteredDefaults {
            models: vec![ModelInfo {
                id: "m1".into(),
                display_name: "Model One".into(),
                description: None,
                available: None,
            }],
            default: Some("m1".into()),
        },
    );
    let models = resolve_models(id);
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "m1");
    assert_eq!(resolve_default(id).as_deref(), Some("m1"));
    registry().write().remove(id);
}

#[test]
fn unregistered_provider_returns_empty() {
    assert!(resolve_models("ghost-never-registered").is_empty());
    assert!(resolve_default("ghost-never-registered").is_none());
}

#[test]
fn override_parser_rejects_unknown_top_level_sections() {
    let error = toml::from_str::<CatalogFile>(
        r#"[[models]]
id = "fixture"
display_name = "Fixture"

[[effrots]]
id = "xhigh"
display_name = "Extra high"
"#,
    )
    .expect_err("unknown override sections must not be silently discarded");

    assert!(error.to_string().contains("unknown field"));
}

#[test]
fn merge_replaces_same_id_keeps_position_and_appends_new() {
    // Built-ins: opus, sonnet, haiku in that order. User override: re-titles sonnet, adds custom finetune. Expectation: order [opus, sonnet-renamed, haiku, my-finetune].
    let builtin = vec![
        ModelInfo {
            id: "opus".into(),
            display_name: "Opus 4.7".into(),
            description: None,
            available: None,
        },
        ModelInfo {
            id: "sonnet".into(),
            display_name: "Sonnet 4.6".into(),
            description: None,
            available: None,
        },
        ModelInfo {
            id: "haiku".into(),
            display_name: "Haiku 4.5".into(),
            description: None,
            available: None,
        },
    ];
    let overrides = vec![
        ModelInfo {
            id: "sonnet".into(),
            display_name: "My Sonnet".into(),
            description: None,
            available: None,
        },
        ModelInfo {
            id: "my-finetune".into(),
            display_name: "Mine".into(),
            description: None,
            available: None,
        },
    ];
    let merged = merge_models(&builtin, &overrides);
    assert_eq!(merged.len(), 4);
    assert_eq!(merged[0].id, "opus");
    assert_eq!(merged[1].id, "sonnet");
    assert_eq!(merged[1].display_name, "My Sonnet"); // override won
    assert_eq!(merged[2].id, "haiku");
    assert_eq!(merged[3].id, "my-finetune");
}

#[test]
fn merge_with_empty_override_is_builtin_verbatim() {
    let builtin = vec![ModelInfo {
        id: "a".into(),
        display_name: "A".into(),
        description: None,
        available: None,
    }];
    let merged = merge_models(&builtin, &[]);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].id, "a");
}

#[test]
fn merge_with_empty_builtin_is_override_verbatim() {
    let overrides = vec![ModelInfo {
        id: "x".into(),
        display_name: "X".into(),
        description: None,
        available: None,
    }];
    let merged = merge_models(&[], &overrides);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].id, "x");
}

#[test]
fn reregistering_unchanged_seed_keeps_live_overlay_changed_seed_replaces_it() {
    let id = "test-reregister-live-overlay";
    cleanup(id);
    let seed = RegisteredDefaults {
        models: vec![model("m1", "Seed")],
        default: Some("m1".into()),
    };
    register(id, seed.clone());
    put_live(id, vec![model("m1", "Live")], Some("m1".into()));

    register(id, seed); // rescan/hot-reload with the identical seed
    assert_eq!(
        resolve_models(id)[0].display_name, "Live",
        "unchanged seed re-registration must not drop a genuine live overlay"
    );

    register(
        id,
        RegisteredDefaults {
            models: vec![model("m1", "Newer Seed")],
            default: Some("m1".into()),
        },
    );
    assert_eq!(
        resolve_models(id)[0].display_name, "Newer Seed",
        "a changed seed invalidates the prior live overlay"
    );
    cleanup(id);
}

#[test]
fn register_twice_replaces() {
    let id = "test-register-twice-replaces";
    register(
        id,
        RegisteredDefaults {
            models: vec![ModelInfo {
                id: "a".into(),
                display_name: "A".into(),
                description: None,
                available: None,
            }],
            default: Some("a".into()),
        },
    );
    register(
        id,
        RegisteredDefaults {
            models: vec![ModelInfo {
                id: "b".into(),
                display_name: "B".into(),
                description: None,
                available: None,
            }],
            default: Some("b".into()),
        },
    );
    let models = resolve_models(id);
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "b");
    registry().write().remove(id);
}

#[test]
fn put_live_overrides_seed_for_models_and_default() {
    let id = "test-put-live-overrides-seed";
    register(
        id,
        RegisteredDefaults {
            models: vec![ModelInfo {
                id: "seed".into(),
                display_name: "Seed".into(),
                description: None,
                available: None,
            }],
            default: Some("seed".into()),
        },
    );

    put_live(
        id,
        vec![ModelInfo {
            id: "live".into(),
            display_name: "Live".into(),
            description: None,
            available: None,
        }],
        Some("live".into()),
    );

    let models = resolve_models(id);
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "live");
    assert_eq!(resolve_default(id).as_deref(), Some("live"));
    registry().write().remove(id);
}

#[test]
fn is_stale_uses_injected_now() {
    let id = "test-is-stale-injected-now";
    register(
        id,
        RegisteredDefaults {
            models: vec![ModelInfo {
                id: "seed".into(),
                display_name: "Seed".into(),
                description: None,
                available: None,
            }],
            default: Some("seed".into()),
        },
    );

    put_live(
        id,
        vec![ModelInfo {
            id: "live".into(),
            display_name: "Live".into(),
            description: None,
            available: None,
        }],
        Some("live".into()),
    );
    let fetched_at = test_live_fetched_at(id);

    assert!(!is_stale(
        id,
        Duration::from_secs(60),
        fetched_at + Duration::from_secs(59)
    ));
    assert!(is_stale(
        id,
        Duration::from_secs(60),
        fetched_at + Duration::from_secs(60)
    ));
    registry().write().remove(id);
}

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
                },
                ModelInfo {
                    id: "gpt-5.5-mini".into(),
                    display_name: "GPT-5.5 Mini".into(),
                    description: None,
                    available: None,
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
                },
                ModelInfo {
                    id: "gpt-5.5-mini".into(),
                    display_name: "GPT-5.5 Mini".into(),
                    description: None,
                    available: None,
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

fn test_live_fetched_at(provider_id: &str) -> SystemTime {
    registry()
        .read()
        .get(provider_id)
        .and_then(|snapshot| snapshot.fetched_at)
        .expect("live snapshot fetched_at")
}

fn model(id: &str, display: &str) -> ModelInfo {
    ModelInfo {
        id: id.into(),
        display_name: display.into(),
        description: None,
        available: None,
    }
}

/// Minimal provider whose `discover_models` returns a canned result, so the
/// refresh path can be driven deterministically (Some = online, None =
/// offline/opt-out).
struct DiscoverStub {
    id: String,
    default: Option<String>,
    discovery: Option<Vec<ModelInfo>>,
}

#[async_trait::async_trait]
impl AgentProvider for DiscoverStub {
    fn id(&self) -> &str {
        &self.id
    }
    fn display_name(&self) -> &str {
        &self.id
    }
    fn build_launch_command(&self, _: &crate::provider::AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> crate::provider::ReadinessStrategy {
        crate::provider::ReadinessStrategy::FixedDelay {
            duration: Duration::ZERO,
        }
    }
    fn task_delivery(&self) -> crate::provider::TaskDelivery {
        crate::provider::TaskDelivery::PtyInput
    }
    fn detect_from_title(&self, _: &str) -> bool {
        false
    }
    fn detect_from_output(&self, _: &str) -> bool {
        false
    }
    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        None
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        None
    }
    fn default_model(&self) -> Option<String> {
        self.default.clone()
    }
    async fn discover_models(&self) -> Option<Vec<ModelInfo>> {
        self.discovery.clone()
    }
}

fn cleanup(id: &str) {
    registry().write().remove(id);
    seed_registry().write().remove(id);
    let _ = std::fs::remove_file(cache_path(id));
}

#[test]
fn put_live_writes_cache_that_survives_restart() {
    let id = "test-cache-survives-restart";
    let _ = std::fs::remove_file(cache_path(id));
    put_live(id, vec![model("live", "Live One")], Some("live".into()));

    let cached = read_cache(id).expect("cache file written");
    assert_eq!(cached.models.len(), 1);
    assert_eq!(cached.models[0].id, "live");
    assert_eq!(cached.default.as_deref(), Some("live"));

    // …and survives a "restart": drop the in-memory snapshot, prime from disk.
    registry().write().remove(id);
    assert!(resolve_models(id).is_empty(), "snapshot cleared");
    prime_from_cache(id);
    let models = resolve_models(id);
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "live");
    assert_eq!(resolve_default(id).as_deref(), Some("live"));
    cleanup(id);
}

#[test]
fn prime_does_not_clobber_a_live_snapshot() {
    let id = "test-prime-no-clobber";
    let _ = std::fs::remove_file(cache_path(id));
    // Persist an OLD cache to disk, then publish a NEWER in-memory snapshot.
    let payload = CachedSnapshot {
        fetched_at_unix: 1,
        default: Some("old".into()),
        models: vec![model("old", "Old")],
        seed_fingerprint: None,
    };
    std::fs::write(cache_path(id), serde_json::to_string(&payload).unwrap()).unwrap();
    put_live(id, vec![model("new", "New")], Some("new".into()));
    prime_from_cache(id); // must NOT replace the live snapshot with stale disk
    assert_eq!(resolve_models(id)[0].id, "new");
    cleanup(id);
}

#[tokio::test]
async fn refresh_models_skips_when_fresh_refreshes_when_forced() {
    let id = "test-refresh-ttl-gate";
    cleanup(id);
    register(
        id,
        RegisteredDefaults {
            models: vec![model("seed", "Seed")],
            default: Some("seed".into()),
        },
    );
    put_live(id, vec![model("old", "Old")], Some("old".into()));

    let stub = Arc::new(DiscoverStub {
        id: id.into(),
        default: Some("new".into()),
        discovery: Some(vec![model("new", "New")]),
    });

    // Fresh snapshot → refresh skipped, list unchanged.
    assert!(refresh_models(stub.clone(), false).await.is_none());
    assert_eq!(resolve_models(id)[0].id, "old");

    // force=true → refreshes regardless of TTL.
    assert!(refresh_models(stub, true).await.is_some());
    assert_eq!(resolve_models(id)[0].id, "new");
    cleanup(id);
}

#[tokio::test]
async fn refresh_live_offline_falls_back_to_seed() {
    let id = "test-refresh-offline-fallback";
    cleanup(id);
    register(
        id,
        RegisteredDefaults {
            models: vec![model("seed", "Seed")],
            default: Some("seed".into()),
        },
    );
    let stub = Arc::new(DiscoverStub {
        id: id.into(),
        default: Some("seed".into()),
        discovery: None, // offline / opt-out
    });
    assert!(refresh_live(stub, Duration::from_secs(1)).await.is_none());
    // Seed remains — list never goes empty.
    let models = resolve_models(id);
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "seed");
    cleanup(id);
}

#[tokio::test]
async fn refresh_live_empty_after_sanitize_keeps_seed() {
    let id = "test-refresh-empty-keeps-seed";
    cleanup(id);
    register(
        id,
        RegisteredDefaults {
            models: vec![model("seed", "Seed")],
            default: Some("seed".into()),
        },
    );
    // Discovery returns only a non-chat (embedding) model → sanitizes to empty.
    let stub = Arc::new(DiscoverStub {
        id: id.into(),
        default: None,
        discovery: Some(vec![model("text-embedding-3", "Embedding")]),
    });
    assert!(refresh_live(stub, Duration::from_secs(1)).await.is_none());
    assert_eq!(resolve_models(id)[0].id, "seed");
    cleanup(id);
}

#[test]
fn legacy_live_cache_without_seed_fingerprint_is_rejected() {
    let id = "test-legacy-cache-no-fingerprint";
    cleanup(id);
    register(
        id,
        RegisteredDefaults {
            models: vec![model("seed", "Seed")],
            default: Some("seed".into()),
        },
    );
    let legacy = serde_json::json!({
        "fetched_at_unix": 1_700_000_000u64,
        "default": "stale",
        "models": [{ "id": "stale", "display_name": "Stale Live" }],
    });
    std::fs::write(cache_path(id), legacy.to_string()).unwrap();

    prime_from_cache(id);

    let models = resolve_models(id);
    assert_eq!(models.len(), 1);
    assert_eq!(
        models[0].id, "seed",
        "cache without a seed fingerprint must never replace the registered seed"
    );
    cleanup(id);
}

#[test]
fn live_cache_from_prior_seed_fingerprint_is_rejected() {
    let id = "test-mismatched-seed-fingerprint";
    cleanup(id);
    register(
        id,
        RegisteredDefaults {
            models: vec![model("opus", "Old Label 4.8")],
            default: Some("opus".into()),
        },
    );
    // The old build persisted a live snapshot fingerprinted against its seed.
    put_live(id, vec![model("opus", "Live From Old Seed")], Some("opus".into()));
    // A new build ships an updated seed and restarts.
    register(
        id,
        RegisteredDefaults {
            models: vec![model("opus", "New Label 5")],
            default: Some("opus".into()),
        },
    );

    prime_from_cache(id);

    assert_eq!(
        resolve_models(id)[0].display_name, "New Label 5",
        "cache captured against a prior seed must be rejected before priming"
    );
    assert!(
        is_stale(id, DEFAULT_TTL, SystemTime::now()),
        "a rejected cache must not carry its TTL forward"
    );
    cleanup(id);
}

#[test]
fn matching_seed_fingerprint_primes_genuine_live_cache() {
    let id = "test-matching-fingerprint-primes";
    cleanup(id);
    let seed = RegisteredDefaults {
        models: vec![model("opus", "Seed")],
        default: Some("opus".into()),
    };
    register(id, seed.clone());
    put_live(
        id,
        vec![ModelInfo {
            id: "opus".into(),
            display_name: "Live".into(),
            description: None,
            available: Some(true),
        }],
        Some("opus".into()),
    );
    // Restart against the SAME seed: startup registers, then warm-starts.
    registry().write().remove(id);
    seed_registry().write().remove(id);
    register(id, seed);
    prime_from_cache(id);

    let models = resolve_models(id);
    assert_eq!(models[0].display_name, "Live");
    assert_eq!(models[0].available, Some(true));
    let fetched_at = test_live_fetched_at(id);
    assert!(!is_stale(id, DEFAULT_TTL, fetched_at + Duration::from_secs(1)));
    cleanup(id);
}

#[tokio::test]
async fn null_discovery_keeps_seed_and_writes_no_live_cache() {
    let id = "test-null-discovery-no-cache";
    cleanup(id);
    register(
        id,
        RegisteredDefaults {
            models: vec![model("seed", "Seed")],
            default: Some("seed".into()),
        },
    );
    let stub = Arc::new(DiscoverStub {
        id: id.into(),
        default: None,
        discovery: None,
    });

    assert!(refresh_live(stub, Duration::from_secs(5)).await.is_none());

    assert!(
        !cache_path(id).exists(),
        "null discovery must not write a live cache file"
    );
    assert_eq!(resolve_models(id)[0].id, "seed");
    assert!(
        is_stale(id, DEFAULT_TTL, SystemTime::now()),
        "null discovery must not start the live TTL"
    );
    cleanup(id);
}

#[test]
fn live_discovery_corrects_stale_seed_label() {
    let id = "test-live-label-correction";
    cleanup(id);
    register(
        id,
        RegisteredDefaults {
            models: vec![model("opus", "Claude Opus 4.7")],
            default: Some("opus".into()),
        },
    );
    // Live discovery surfaces a fresher label for the same alias id.
    put_live(
        id,
        vec![model("opus", "Claude Opus 4.8")],
        Some("opus".into()),
    );
    assert_eq!(resolve_models(id)[0].display_name, "Claude Opus 4.8");
    cleanup(id);
}

#[test]
fn availability_flag_is_carried_through_resolve() {
    let id = "test-availability-flag";
    cleanup(id);
    register(
        id,
        RegisteredDefaults {
            models: vec![
                model("opus", "Opus"),
                ModelInfo {
                    id: "fable".into(),
                    display_name: "Fable".into(),
                    description: None,
                    available: Some(false),
                },
            ],
            default: Some("opus".into()),
        },
    );
    let fable = resolve_models(id)
        .into_iter()
        .find(|m| m.id == "fable")
        .expect("fable present");
    assert_eq!(fable.available, Some(false));
    cleanup(id);
}

#[test]
fn sanitize_live_drops_non_chat_and_dedups() {
    let cleaned = sanitize_live(vec![
        model("gpt-chat", "Chat"),
        model("text-embedding-3-large", "Embedding"),
        model("", "Empty"),
        model("gpt-chat", "Dup"),
    ]);
    assert_eq!(cleaned.len(), 1);
    assert_eq!(cleaned[0].id, "gpt-chat");
}

#[test]
fn resolve_live_default_recovers_when_default_vanished() {
    let models = vec![model("b", "B"), model("c", "C")];
    // Hardcoded default "a" gone from live → fall back to first live id.
    assert_eq!(
        resolve_live_default(&models, Some("a".into())).as_deref(),
        Some("b")
    );
    // Default still present → kept.
    assert_eq!(
        resolve_live_default(&models, Some("c".into())).as_deref(),
        Some("c")
    );
}
