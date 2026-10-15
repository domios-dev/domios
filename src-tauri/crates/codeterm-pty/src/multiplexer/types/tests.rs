use super::*;

#[test]
fn staged_model_selection_is_visible_but_never_restores_execution() {
    let mut tab = tab_with("pending-model-tab", TabBacking::Pty);
    tab.pending_model_selection = Some(PendingModelSelection {
        model: "selected-model".into(),
        effort: Some("selected-effort".into()),
    });
    let snapshot = serde_json::to_value(&tab).unwrap();
    assert_eq!(
        snapshot["pending_model_selection"]["model"],
        "selected-model"
    );
    assert_eq!(
        snapshot["pending_model_selection"]["effort"],
        "selected-effort"
    );
    let restored: Tab = serde_json::from_value(snapshot).unwrap();
    assert!(restored.pending_model_selection.is_none());
}

fn tab_with(id: &str, backing: TabBacking) -> Tab {
    Tab {
        id: id.to_string(),
        sort_order: 0,
        pty_id: None,
        backing,
        tab_type: TabType::Terminal,
        purpose: Default::default(),
        icon: None,
        shell_profile: None,
        utility_created_at: 0,
        utility_command_pending: false,
        close_restore: None,
        shell_command_state: Default::default(),
        pending_delivery: None,
        title: "Terminal".to_string(),
        description: None,
        cwd: None,
        provisioned_cwd: None,
        url: None,
        agent_type: None,
        agent_type_locked: false,
        agent_session_id: None,
        agent_session_name: None,
        context: None,
        provider_signals: Default::default(),
        fork_source_session_id: None,
        agent_session_model: None,
        agent_session_model_identity: None,
        pending_model_switch_observation: None,
        pending_model_selection: None,
        agent_planned_model: None,
        agent_requested_model: None,
        structured_control_active: false,
        agent_runtime_state: None,
        agent_runtime_config: None,
        agent_runtime_questions: None,
        structured_runtime_mode: None,
        agent_provider_resolved_model: None,
        model_fallback: None,
        model_identity_mismatch: None,
        reasoning_effort_identity: ReasoningEffortIdentity::default(),
        restorable_launch: RestorableLaunchParameters::default(),
        context_epoch_archives: vec![],
        session_binding_nonce: None,
        reported_session_binding_nonce: None,
        prior_session_binding_nonces: Vec::new(),
        shell_integration_nonce: None,
        lifecycle_epoch: None,
        lifecycle_operation_id: None,
        task_binding: None,
        roleless_tracked: false,
        managed: None,
        role: None,
        role_profile: None,
        team_id: None,
        watcher: None,
        parent_tab_id: None,
        pending_paired_planner_tab_id: None,
        can_spawn: false,
        depth: 0,
        report_target_tab_id: None,
        report_target_mesh_peer: None,
        spawned_by_tab_id: None,
        ownership: None,
        title_level: TitleLevel::Default,
        chat_view: false,
        pending_prompt: None,
        pending_prompt_kind: None,
        agent_state: None,
        turn_resume: None,
        auto_approve: false,
        permission_auto_confirm: false,
        permission_auto_confirmations: Vec::new(),
        protected: false,
        connected_services: vec![],
        agent_set_at: None,
        exit_suspected_at: None,
        agent_booting: false,
        hidden: false,
        audience: Default::default(),
        show_in_tabbar: true,
        worker_id: None,
        last_active_at_ms: None,
        restored_from_snapshot: false,
        scrollback_seed_pending: false,
        pending_scrollback_tail: None,
        skip_snapshot: false,
        pending_greeting: None,
        boot_stage: None,
        boot_failure_reason: None,
        provisional_launch: None,
    }
}

#[test]
fn pty_tab_is_not_chat_backend() {
    let p = tab_with("a", TabBacking::Pty);
    assert!(!p.is_chat_backend());
}

#[test]
fn accepted_weak_is_terminal() {
    let mut delivery =
        PendingDeliveryRecord::new("digest".into(), PendingDeliveryChannel::InteractiveInput);
    delivery
        .begin_attempt(DeliveryAttemptProvenance {
            attempt_id: "attempt-1".into(),
            started_at_ms: 42,
        })
        .unwrap();
    assert_eq!(delivery.confirm(), Ok(()));
    assert_eq!(delivery.state(), PendingDeliveryState::AcceptedWeak);
    assert_eq!(
        delivery.begin_attempt(DeliveryAttemptProvenance {
            attempt_id: "attempt-2".into(),
            started_at_ms: 43,
        }),
        Err(PendingDeliveryTransitionError::AcceptedTerminal)
    );
    assert_eq!(
        delivery.confirm(),
        Err(PendingDeliveryTransitionError::AcceptedTerminal)
    );
}

#[test]
fn pending_delivery_uses_verdict_tier_labels() {
    let mut weak =
        PendingDeliveryRecord::new("weak".into(), PendingDeliveryChannel::InteractiveInput);
    weak.mark_ingested();
    assert_eq!(weak.state(), PendingDeliveryState::Ingested);
    weak.mark_unconfirmed();
    assert_eq!(weak.state(), PendingDeliveryState::Unconfirmed);
    weak.confirm().unwrap();
    assert_eq!(weak.state(), PendingDeliveryState::AcceptedWeak);

    let mut strong =
        PendingDeliveryRecord::new("strong".into(), PendingDeliveryChannel::InteractiveInput);
    strong.confirm_strong().unwrap();
    assert_eq!(strong.state(), PendingDeliveryState::AcceptedStrong);
}

#[test]
fn legacy_pending_delivery_defaults_provenance_from_channel() {
    let record: PendingDeliveryRecord = serde_json::from_str(
        r#"{
            "content_digest":"legacy",
            "channel":"fallback_bootstrap",
            "state":"pending",
            "attempt_provenance":null
        }"#,
    )
    .unwrap();
    assert_eq!(record.channel, PendingDeliveryChannel::FallbackBootstrap);
    assert_eq!(
        record.provenance,
        PendingDeliveryProvenance::FallbackBootstrap
    );
    assert_eq!(record.send_id(), None);
    assert!(!record.record_id().is_empty());
}

#[test]
fn pending_interactive_send_round_trips_its_chat_send_id() {
    let record =
        PendingDeliveryRecord::new("digest".into(), PendingDeliveryChannel::InteractiveInput)
            .with_send_id("send-1".into());

    let encoded = serde_json::to_string(&record).unwrap();
    let decoded: PendingDeliveryRecord = serde_json::from_str(&encoded).unwrap();

    assert_eq!(decoded.send_id(), Some("send-1"));
    assert_eq!(decoded.record_id(), record.record_id());
}

#[test]
fn pending_allows_one_inflight_attempt() {
    let mut delivery =
        PendingDeliveryRecord::new("digest".into(), PendingDeliveryChannel::InteractiveInput);
    delivery
        .begin_attempt(DeliveryAttemptProvenance {
            attempt_id: "attempt-1".into(),
            started_at_ms: 42,
        })
        .unwrap();
    assert_eq!(
        delivery.begin_attempt(DeliveryAttemptProvenance {
            attempt_id: "attempt-2".into(),
            started_at_ms: 43,
        }),
        Err(PendingDeliveryTransitionError::AttemptAlreadyInFlight)
    );
}

#[test]
fn mid_boot_snapshot_exposes_current_stage() {
    use codeterm_types::pty::BootStage;
    // the live snapshot.
    let mut p = tab_with("a", TabBacking::Pty);
    p.boot_stage = Some(BootStage::AgentLaunching);
    let v = serde_json::to_value(&p).unwrap();
    assert_eq!(v["boot_stage"], "agent_launching");
    assert!(v.as_object().unwrap().get("boot_failure_reason").is_none());

    // Before any boot progress the field is omitted entirely (skip_if None).
    let idle = tab_with("b", TabBacking::Pty);
    let v = serde_json::to_value(&idle).unwrap();
    assert!(v.as_object().unwrap().get("boot_stage").is_none());
}

#[test]
fn chat_backend_tab_is_detected() {
    let p = tab_with(
        "b",
        TabBacking::ChatBackend {
            plugin_id: "local-shell".into(),
            session_id: "s1".into(),
        },
    );
    assert!(p.is_chat_backend());
}

#[test]
fn backing_round_trips_on_tab_and_defaults_to_pty() {
    // A ChatBackend tab survives a serde round-trip.
    let p = tab_with(
        "c",
        TabBacking::ChatBackend {
            plugin_id: "local-shell".into(),
            session_id: "s1".into(),
        },
    );
    let v = serde_json::to_value(&p).unwrap();
    let back: Tab = serde_json::from_value(v).unwrap();
    assert!(back.is_chat_backend());
    // snapshot) defaults to Pty.
    let mut v = serde_json::to_value(&p).unwrap();
    v.as_object_mut().unwrap().remove("backing");
    let back: Tab = serde_json::from_value(v).unwrap();
    assert_eq!(back.backing, TabBacking::Pty);
    assert!(!back.is_chat_backend());
}

#[test]
fn chat_backing_round_trips_through_serde() {
    let backing = TabBacking::ChatBackend {
        plugin_id: "local-shell".into(),
        session_id: "abc".into(),
    };
    let v = serde_json::to_value(&backing).unwrap();
    assert_eq!(v["kind"], "chat_backend");
    let back: TabBacking = serde_json::from_value(v).unwrap();
    assert_eq!(back, backing);
}

#[test]
fn watcher_policy_clamped_applies_defaults_and_bounds() {
    let all_none = WatcherPolicy::clamped(None, None, None);
    assert_eq!(all_none.tick_interval_secs, 120);
    assert_eq!(all_none.nudge_cooldown_secs, 600);
    assert_eq!(all_none.max_actions_per_tick, 3);

    let low = WatcherPolicy::clamped(Some(5), Some(0), Some(99));
    assert_eq!(low.tick_interval_secs, 30);
    assert_eq!(low.nudge_cooldown_secs, 60);
    assert_eq!(low.max_actions_per_tick, 5);
}

#[test]
fn tab_with_watcher_round_trips_through_serde() {
    let spec = WatcherSpec {
        charter: "Observe the group.".into(),
        policy: WatcherPolicy::clamped(Some(90), Some(120), Some(2)),
        observations: serde_json::json!({"orchestration": true, "chatTail": 2}),
        state: serde_json::json!({"seen": 1}),
        tick: 3,
        last_tick_ms: Some(1_700_000_000_000),
        last_status: Some("ok".into()),
        tick_inflight_since_ms: None,
    };
    let mut p = tab_with("w1", TabBacking::Pty);
    p.watcher = Some(spec.clone());

    let v = serde_json::to_value(&p).unwrap();
    assert!(v.get("watcher").is_some());
    let back: Tab = serde_json::from_value(v).unwrap();
    assert_eq!(back.watcher, Some(spec));

    let mut bare = tab_with("w2", TabBacking::Pty);
    let v = serde_json::to_value(&bare).unwrap();
    assert!(v.get("watcher").is_none());
    bare.watcher = Some(WatcherSpec {
        charter: "x".into(),
        ..Default::default()
    });
    let back: Tab = serde_json::from_value(serde_json::to_value(&bare).unwrap()).unwrap();
    assert_eq!(back.watcher.as_ref().map(|w| w.charter.as_str()), Some("x"));
}

#[test]
fn backing_matches_frontend_ts_contract_shape() {
    // The TS type is `{ kind: 'chat_backend'; plugin_id: string;
    let v = serde_json::to_value(TabBacking::ChatBackend {
        plugin_id: "lmstudio".into(),
        session_id: "s7".into(),
    })
    .unwrap();
    assert_eq!(
        v,
        serde_json::json!({
            "kind": "chat_backend",
            "plugin_id": "lmstudio",
            "session_id": "s7",
        })
    );
    // "not a chat Tab".
    assert_eq!(
        serde_json::to_value(TabBacking::Pty).unwrap(),
        serde_json::json!({ "kind": "pty" })
    );
}

fn surface_backing(owner_label: bool) -> TabBacking {
    TabBacking::Surface(SurfaceBacking {
        plugin_id: "dash".into(),
        surface_id: "board".into(),
        instance_id: "inst-1".into(),
        owner_label,
        resume_url: None,
        resume_profile: None,
    })
}

#[test]
fn surface_backing_round_trips_through_serde_with_surface_tag() {
    for owner_label in [false, true] {
        let backing = surface_backing(owner_label);
        let value = serde_json::to_value(&backing).unwrap();
        assert_eq!(value["kind"], "surface");
        assert_eq!(value["plugin_id"], "dash");
        assert_eq!(value["surface_id"], "board");
        assert_eq!(value["instance_id"], "inst-1");
        assert_eq!(value.get("owner_label").is_some(), owner_label);
        let back: TabBacking = serde_json::from_value(value).unwrap();
        assert_eq!(back, backing);
    }
}

#[test]
fn surface_tab_round_trips_and_is_neither_pty_nor_chat() {
    let tab = tab_with("s", surface_backing(true));
    let back: Tab = serde_json::from_value(serde_json::to_value(&tab).unwrap()).unwrap();
    assert_eq!(back.backing, surface_backing(true));
    assert_eq!(back.surface().map(|s| s.surface_id.as_str()), Some("board"));
    assert!(!back.is_chat_backend());
    assert!(!back.is_utility_shell());
    assert_eq!(back.backing.kind(), "surface");
    assert_eq!(TabBacking::Pty.kind(), "pty");
}

#[test]
fn only_surface_children_close_with_their_owner() {
    assert!(tab_with("s", surface_backing(false)).closes_with_owner());
    assert!(!tab_with("p", TabBacking::Pty).closes_with_owner());
    assert!(!tab_with(
        "c",
        TabBacking::ChatBackend {
            plugin_id: "chat".into(),
            session_id: "s".into(),
        },
    )
    .closes_with_owner());
}
