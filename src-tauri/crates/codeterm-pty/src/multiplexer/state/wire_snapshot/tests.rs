use super::super::MultiplexerState;
use crate::multiplexer::types::{ExitedChatBinding, PriorBindingNonce, RestorableLaunchParameters};
use crate::session::store::SessionSnapshot;
use serde_json::json;

#[test]
fn exited_chat_recipient_round_trips_independently_of_the_shell_nonce() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.prior_session_binding_nonces.push(PriorBindingNonce {
        nonce: "retired-provider".into(),
        session_id: Some("own-session".into()),
        resume_target_session_id: None,
    });
    tab.restorable_launch.exited_chat_binding = Some(ExitedChatBinding {
        provider_id: "fixture".into(),
        nonce: "retired-provider".into(),
        session_id: "own-session".into(),
    });
    let mut restored = MultiplexerState::from_session_snapshot(&mux.to_session_snapshot());
    restored.get_tab_mut(&id).unwrap().session_binding_nonce = Some("new-shell".into());
    let wire = serde_json::to_value(restored.snapshot()).unwrap();
    assert_eq!(
        wire["tabs"][&id]["exited_chat_recipient"],
        json!({
            "provider_id":"fixture", "nonce":"retired-provider", "session_id":"own-session"
        })
    );
    restored
        .get_tab_mut(&id)
        .unwrap()
        .restorable_launch
        .exited_chat_binding = None;
    let stopped = serde_json::to_value(restored.snapshot()).unwrap();
    assert!(stopped["tabs"][&id].get("exited_chat_recipient").is_none());
    assert_eq!(
        stopped["tabs"][&id]["prior_session_binding_nonces"][0]["session_id"],
        "own-session"
    );
}

#[test]
fn wire_account_projection_tracks_authoritative_launch_and_restore() {
    let (mut state, id) = MultiplexerState::new();
    let wire = serde_json::to_value(state.snapshot()).unwrap();
    assert_eq!(
        wire["tabs"][&id]["account"],
        json!({"id":"default", "notices":[]})
    );
    populate(&mut state, &id, 4);
    let launch = &mut state.get_tab_mut(&id).unwrap().restorable_launch;
    launch.account_id = Some("work".into());
    launch.account_namespace = Some("private-namespace".into());
    launch
        .account_notices
        .push(crate::multiplexer::types::AccountContinuationNotice {
            verification: None,
            id: "receipt".into(),
            provider: "fixture-provider".into(),
            session: "own-session".into(),
            at_ms: 12,
            content: "Continued".into(),
            boundary: Some("own-turn".into()),
            resets_at_ms: None,
        });
    let saved = state.to_session_snapshot();
    let restored = MultiplexerState::from_session_snapshot(&saved);
    let mut tab = serde_json::to_value(restored.get_tab(&id).unwrap()).unwrap();
    let projection = &tab["account"];
    assert_eq!(projection.as_object().unwrap().len(), 3);
    assert_eq!(projection["id"], "work");
    assert_eq!(projection["namespace"], "private-namespace");
    assert_eq!(projection["notices"][0]["id"], "receipt");
    assert!(tab.get("restorable_launch").is_none());
    assert!(!tab.to_string().contains("identity_instructions"));
    tab["account"] = json!({"id":"foreign", "namespace":"foreign", "notices":[]});
    let read: crate::multiplexer::types::Tab = serde_json::from_value(tab).unwrap();
    assert_eq!(
        read.restorable_launch,
        RestorableLaunchParameters::default()
    );
    state.get_tab_mut(&id).unwrap().restorable_launch.account_id = Some("next".into());
    let wire = serde_json::to_value(state.snapshot()).unwrap();
    assert_eq!(wire["tabs"][&id]["account"]["id"], "next");
}

fn populate(state: &mut MultiplexerState, id: &str, config: usize) {
    let tab = state.get_tab_mut(id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.agent_runtime_config = Some(json!({
        "systemPrompt": format!("{config}:{}", "p".repeat(42_000)),
        "model": format!("model-{config}"),
        "options": {"temperature": 0.5}
    }));
    tab.restorable_launch = RestorableLaunchParameters {
        captured: true,
        model: Some(format!("model-{config}")),
        identity_instructions: Some("i".repeat(1_250)),
        ..Default::default()
    };
    tab.prior_session_binding_nonces = (0..8)
        .map(|n| PriorBindingNonce {
            session_id: Some(format!("session-{n}")),
            resume_target_session_id: None,
            nonce: format!("nonce-{n}"),
        })
        .collect();
}

#[test]
fn wire_snapshot_140_tabs_stays_below_450kb() {
    let (mut state, first) = MultiplexerState::new();
    let workspace = state.list_workspaces()[0].id.clone();
    populate(&mut state, &first, 0);
    for n in 1..140 {
        let id = state.create_tab(&workspace).unwrap();
        populate(&mut state, &id, n % 12);
    }
    for n in 0..100 {
        let id = state.create_tab(&workspace).unwrap();
        populate(&mut state, &id, n % 12);
        state.close_tab(&id).unwrap();
    }
    let encoded = serde_json::to_vec(&state.snapshot()).unwrap();
    assert!(encoded.len() < 450_000, "{} bytes", encoded.len());
    let wire: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
    let tabs = wire["tabs"].as_object().unwrap();
    assert_eq!(tabs.len(), 140);
    for tab in tabs.values() {
        let config = tab["agent_runtime_config"].as_object().unwrap();
        assert!(!config.contains_key("systemPrompt"));
        assert_eq!(config["options"], json!({"temperature": 0.5}));
        let nonces = tab["prior_session_binding_nonces"].as_array().unwrap();
        assert_eq!(nonces.len(), 2);
        assert_eq!(nonces[0]["nonce"], "nonce-6");
        assert_eq!(nonces[1]["nonce"], "nonce-7");
    }
    let closed = wire["recently_closed"].as_array().unwrap();
    assert_eq!(closed.len(), 100);
    assert!(closed
        .iter()
        .all(|tab| tab.get("restorable_launch").is_none()));
    assert!(!String::from_utf8(encoded).unwrap().contains("systemPrompt"));
}

#[test]
fn wire_serialization_preserves_persisted_runtime_launch_and_nonce_history() {
    let (mut state, id) = MultiplexerState::new();
    populate(&mut state, &id, 4);
    let original = state.get_tab(&id).unwrap().clone();
    serde_json::to_vec(&state.snapshot()).unwrap();
    let saved = serde_json::to_vec(&state.to_session_snapshot()).unwrap();
    let loaded: SessionSnapshot = serde_json::from_slice(&saved).unwrap();
    let restored = MultiplexerState::from_session_snapshot(&loaded);
    let tab = restored.get_tab(&id).unwrap();
    assert_eq!(tab.agent_runtime_config, original.agent_runtime_config);
    assert_eq!(tab.restorable_launch, original.restorable_launch);
    assert_eq!(
        tab.prior_session_binding_nonces,
        original.prior_session_binding_nonces
    );
    assert_eq!(tab.prior_session_binding_nonces.len(), 8);
}

#[test]
fn wire_serialization_preserves_server_side_closed_tab_restore() {
    let (mut state, id) = MultiplexerState::new();
    let workspace = state.list_workspaces()[0].id.clone();
    populate(&mut state, &id, 7);
    let launch = state.get_tab(&id).unwrap().restorable_launch.clone();
    state.close_tab(&id).unwrap();
    let wire = serde_json::to_value(state.snapshot()).unwrap();
    assert!(wire["recently_closed"][0]
        .get("restorable_launch")
        .is_none());
    assert_eq!(
        state.list_recently_closed(&workspace)[0].restorable_launch,
        launch
    );
    let restored = state.reopen_closed_tab(&workspace, None).unwrap();
    assert_eq!(state.get_tab(&restored).unwrap().restorable_launch, launch);
}

#[test]
fn wire_cloud_mode_is_the_exact_persisted_launch_and_survives_restore() {
    let (mut state, id) = MultiplexerState::new();
    let tab = state.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("opaque-provider".into());
    tab.restorable_launch.execution_mode = Some("opaque-cloud".into());
    let encoded = serde_json::to_string(&state.to_session_snapshot()).unwrap();
    let saved: SessionSnapshot = serde_json::from_str(&encoded).unwrap();
    let restored = MultiplexerState::from_session_snapshot(&saved);
    assert_eq!(
        restored
            .get_tab(&id)
            .unwrap()
            .restorable_launch
            .execution_mode
            .as_deref(),
        Some("opaque-cloud")
    );
    let wire = serde_json::to_value(restored.snapshot()).unwrap();
    assert_eq!(
        wire["tabs"][&id]["agent_runtime_config"]["executionMode"],
        "opaque-cloud"
    );
}

#[test]
fn closed_summary_matches_the_redacted_persisted_record() {
    let (mut state, id) = MultiplexerState::new();
    populate(&mut state, &id, 3);
    state.get_tab_mut(&id).unwrap().backing =
        crate::multiplexer::types::TabBacking::Surface(crate::multiplexer::types::SurfaceBacking {
            plugin_id: "plugin".into(),
            surface_id: "surface".into(),
            instance_id: "instance".into(),
            owner_label: true,
            resume_url: None,
            resume_profile: None,
        });
    state.close_tab(&id).unwrap();
    let snapshot = state.snapshot();
    let view = &snapshot.recently_closed[0];
    let workspace = state.list_workspaces()[0].id.clone();
    let record = &state.list_recently_closed(&workspace)[0];
    let mut previous = serde_json::to_value(record).unwrap();
    previous
        .as_object_mut()
        .unwrap()
        .remove("restorable_launch");
    assert_eq!(serde_json::to_value(&view.tab).unwrap(), previous);
    assert!(previous.get("description").is_none());
    assert!(previous.get("agent_runtime_config").is_none());
}

fn runtime_state_fixture(model: &str, command_count: usize, icon_svg: &str) -> serde_json::Value {
    let commands: Vec<serde_json::Value> = (0..command_count)
        .map(|i| {
            json!({
                "name": format!("cmd-{i}"),
                "description": format!("Static provider command description {i} {}", "x".repeat(120)),
                "argumentHint": "[arg]"
            })
        })
        .collect();
    json!({
        "state": "loaded",
        "epoch": 2,
        "agentName": "Fixture Runtime",
        "binaryVersion": "1.0.0",
        "iconSvg": icon_svg,
        "authentication": {
            "state": "authenticated",
            "methods": [{"id": "oauth", "name": "OAuth", "available": true}]
        },
        "controls": {
            "model": model,
            "mode": "code",
            "commands": commands,
            "modes": [
                {"id": "code", "name": "Code"},
                {"id": "ask", "name": "Ask"}
            ],
            "configOptions": [
                {
                    "id": "model",
                    "name": "Model",
                    "category": "model",
                    "currentValue": model,
                    "options": [
                        {"value": "model-alpha", "name": "Alpha", "description": "Primary model"},
                        {"value": "model-beta", "name": "Beta", "description": "Secondary model"}
                    ]
                },
                {
                    "id": "thought_level",
                    "name": "Thought level",
                    "category": "thought_level",
                    "currentValue": "high",
                    "options": [
                        {"value": "low", "name": "Low"},
                        {"value": "high", "name": "High"}
                    ]
                }
            ]
        }
    })
}

fn service_tier_catalog(variant: &str) -> serde_json::Value {
    json!({
        "model-a": [
            {"id": "fast", "name": "Fast", "description": format!("Fast tier {variant} {}", "t".repeat(180))},
            {"id": "standard", "name": "Standard", "description": format!("Standard tier {variant} {}", "s".repeat(180))}
        ],
        "model-b": [
            {"id": "fast", "name": "Fast", "description": format!("Fast tier B {variant} {}", "u".repeat(180))}
        ]
    })
}

#[test]
fn runtime_controls_and_icons_are_hoisted_once_per_unique_payload_and_preserve_per_tab_values() {
    let (mut state, first) = MultiplexerState::new();
    let workspace = state.list_workspaces()[0].id.clone();
    let svg = format!(
        "<svg viewBox=\"0 0 24 24\"><path d=\"{}\"/></svg>",
        "M1 1 ".repeat(350)
    );

    let mut primary_tabs = vec![first];
    for _ in 1..20 {
        primary_tabs.push(state.create_tab(&workspace).unwrap());
    }
    for (idx, id) in primary_tabs.iter().enumerate() {
        let tab = state.get_tab_mut(id).unwrap();
        tab.agent_type = Some("fixture-runtime-a".into());
        let model = if idx % 2 == 0 {
            "model-alpha"
        } else {
            "model-beta"
        };
        // Two workspace command sets: 12 tabs with 30 commands, 8 tabs with 10 commands.
        let cmd_count = if idx < 12 { 30 } else { 10 };
        tab.agent_runtime_state = Some(runtime_state_fixture(model, cmd_count, &svg));
    }

    let mut tiered_tabs = Vec::new();
    for idx in 0..10 {
        let id = state.create_tab(&workspace).unwrap();
        let tab = state.get_tab_mut(&id).unwrap();
        tab.agent_type = Some("fixture-runtime-b".into());
        let tiers = service_tier_catalog(if idx < 8 { "acct-a" } else { "acct-b" });
        tab.agent_runtime_config = Some(json!({
            "model": "model-a",
            "serviceTierAvailability": tiers.clone()
        }));
        tab.agent_runtime_state = Some(json!({
            "state": "loaded",
            "epoch": 1,
            "controls": {
                "model": "model-a",
                "serviceTierAvailability": tiers
            }
        }));
        tiered_tabs.push(id);
    }

    let raw_unhoisted_bytes: usize = state
        .snapshot()
        .tabs
        .values()
        .map(|tab| {
            serde_json::to_vec(&tab.agent_runtime_state).unwrap().len()
                + serde_json::to_vec(&tab.agent_runtime_config).unwrap().len()
        })
        .sum();
    let encoded = serde_json::to_vec(&state.snapshot()).unwrap();
    // 20 primary tabs + 10 tiered tabs would exceed 160 KB inline; hoisted stays well below half.
    assert!(
        encoded.len() < raw_unhoisted_bytes / 2,
        "wire={} bytes vs raw_runtime={} bytes",
        encoded.len(),
        raw_unhoisted_bytes
    );

    let wire: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
    let assets = wire["runtime_assets"].as_object().unwrap();
    assert_eq!(assets["icons"].as_object().unwrap().len(), 1);
    assert_eq!(assets["authentication"].as_object().unwrap().len(), 1);
    assert_eq!(assets["commands"].as_object().unwrap().len(), 2);
    assert_eq!(assets["modes"].as_object().unwrap().len(), 1);
    assert_eq!(assets["configOptions"].as_object().unwrap().len(), 1);
    // Both agent_runtime_state.controls and agent_runtime_config share the exact 2 tier tables.
    assert_eq!(
        assets["serviceTierAvailability"].as_object().unwrap().len(),
        2
    );

    // Every primary tab carries refs + its own configOptionValues, never inline static blobs.
    for (idx, id) in primary_tabs.iter().enumerate() {
        let ars = wire["tabs"][id]["agent_runtime_state"].as_object().unwrap();
        assert!(ars.get("iconSvg").is_none());
        assert!(ars.get("authentication").is_none());
        let icon_ref = ars["iconRef"].as_str().unwrap();
        assert_eq!(assets["icons"][icon_ref], svg);
        let controls = ars["controls"].as_object().unwrap();
        assert!(controls.get("commands").is_none());
        assert!(controls.get("modes").is_none());
        assert!(controls.get("configOptions").is_none());
        let expected_model = if idx % 2 == 0 {
            "model-alpha"
        } else {
            "model-beta"
        };
        assert_eq!(
            controls["configOptionValues"],
            json!([expected_model, "high"])
        );
        let opts_ref = controls["configOptionsRef"].as_str().unwrap();
        assert!(assets["configOptions"][opts_ref][0]
            .get("currentValue")
            .is_none());
    }

    // Tiered tabs share the same serviceTierAvailabilityRef across controls and config.
    for id in &tiered_tabs {
        let ars_ref = wire["tabs"][id]["agent_runtime_state"]["controls"]
            ["serviceTierAvailabilityRef"]
            .as_str()
            .unwrap();
        let cfg_ref = wire["tabs"][id]["agent_runtime_config"]["serviceTierAvailabilityRef"]
            .as_str()
            .unwrap();
        assert_eq!(ars_ref, cfg_ref);
        assert!(wire["tabs"][id]["agent_runtime_state"]["controls"]
            .get("serviceTierAvailability")
            .is_none());
        assert!(wire["tabs"][id]["agent_runtime_config"]
            .get("serviceTierAvailability")
            .is_none());
    }
}

#[test]
fn empty_runtime_assets_are_not_hoisted_and_persisted_session_snapshot_stays_inline() {
    let (mut state, id) = MultiplexerState::new();
    let tab = state.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("fixture-runtime-a".into());
    tab.agent_runtime_state = Some(json!({
        "state": "loaded",
        "epoch": 1,
        "iconSvg": "",
        "authentication": {},
        "controls": {
            "commands": [],
            "modes": [],
            "configOptions": [],
            "serviceTierAvailability": {}
        }
    }));
    let wire = serde_json::to_value(state.snapshot()).unwrap();
    assert!(wire.get("runtime_assets").is_none());
    assert_eq!(wire["tabs"][&id]["agent_runtime_state"]["iconSvg"], "");
    assert_eq!(
        wire["tabs"][&id]["agent_runtime_state"]["controls"]["commands"],
        json!([])
    );

    // Populated runtime state still persists inline in SessionSnapshot for restore/inspect.
    let full = runtime_state_fixture("model-alpha", 4, "<svg/>");
    state.get_tab_mut(&id).unwrap().agent_runtime_state = Some(full.clone());
    let persisted = serde_json::to_vec(&state.to_session_snapshot()).unwrap();
    let loaded: SessionSnapshot = serde_json::from_slice(&persisted).unwrap();
    let restored = MultiplexerState::from_session_snapshot(&loaded);
    assert_eq!(
        restored.get_tab(&id).unwrap().agent_runtime_state,
        Some(full)
    );
}

#[test]
fn runtime_asset_keys_are_deterministic_and_independent_of_sibling_tabs() {
    let (mut state, first) = MultiplexerState::new();
    let workspace = state.list_workspaces()[0].id.clone();
    let second = state.create_tab(&workspace).unwrap();
    let svg = "<svg><circle r=\"4\"/></svg>";
    state.get_tab_mut(&first).unwrap().agent_runtime_state =
        Some(runtime_state_fixture("model-alpha", 5, svg));
    state.get_tab_mut(&second).unwrap().agent_runtime_state =
        Some(runtime_state_fixture("model-beta", 5, svg));

    let before = serde_json::to_value(state.snapshot()).unwrap();
    let second_ref_before = before["tabs"][&second]["agent_runtime_state"]["iconRef"]
        .as_str()
        .unwrap()
        .to_owned();
    let second_cmds_before = before["tabs"][&second]["agent_runtime_state"]["controls"]
        ["commandsRef"]
        .as_str()
        .unwrap()
        .to_owned();

    state.close_tab(&first).unwrap();
    let after = serde_json::to_value(state.snapshot()).unwrap();
    assert_eq!(
        after["tabs"][&second]["agent_runtime_state"]["iconRef"],
        second_ref_before
    );
    assert_eq!(
        after["tabs"][&second]["agent_runtime_state"]["controls"]["commandsRef"],
        second_cmds_before
    );
}
