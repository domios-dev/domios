use super::{
    reconcile_agent_levels, shipped_sound, AgentLevelChoice, IntegrationsConfig, MemoryConfig,
    NotificationConfig, OrchestrationConfig, PushChannelConfig, UiConfig, UsageConfig,
};
use crate::notification::{NotifyDestination, NotifyLevel};

/// A channel id core has never heard of. Every column assertion below routes
/// through one of these, so nothing here can pass by knowing a real channel.
const FIXTURE_CHANNEL: &str = "fixture";

fn fixture_columns() -> Vec<NotifyDestination<'static>> {
    NotifyDestination::columns([FIXTURE_CHANNEL])
}

#[test]
fn push_channels_are_opt_out_on_a_fresh_install() {
    let cfg = NotificationConfig::default();
    assert!(
        cfg.channels.is_empty(),
        "a fresh install names no audience, so nobody receives anything"
    );
    assert!(PushChannelConfig::default().per_scope_limit > 0);
    assert!(
        PushChannelConfig::default().recipients.is_empty(),
        "an entry that named nobody stays nobody; there is no second opt-in to read"
    );

    let from_empty_toml: NotificationConfig = toml::from_str("").unwrap();
    assert!(from_empty_toml.channels.is_empty());
}

#[test]
fn push_channel_recipients_accept_bare_and_quoted_chat_ids() {
    let toml = r#"
        [channels.fixture]
        recipients = [100000001, "789012345"]
    "#;
    let cfg: NotificationConfig = toml::from_str(toml).unwrap();
    let channel = cfg.channels.get(FIXTURE_CHANNEL).expect("the keyed entry");
    assert_eq!(
        channel.recipients,
        vec!["100000001".to_string(), "789012345".to_string()]
    );
    assert_eq!(
        channel.per_scope_limit,
        PushChannelConfig::default().per_scope_limit,
        "an omitted ceiling falls back to the default rather than to zero"
    );
}

#[test]
fn ui_config_extra_keys_round_trip_over_json() {
    // talks JSON over IPC, so the JSON round-trip is the load-bearing path.
    let json = r#"{
        "usage_intro_hint_opens": 3
    }"#;
    let cfg: UiConfig = serde_json::from_str(json).unwrap();
    assert_eq!(
        cfg.extra.get("usage_intro_hint_opens"),
        Some(&toml::Value::Integer(3))
    );

    let back = serde_json::to_value(&cfg).unwrap();
    assert_eq!(back["usage_intro_hint_opens"], serde_json::json!(3));
}

#[test]
fn ui_config_extra_survives_toml_round_trip() {
    let toml_src = "keepawake_display = true\n";
    let cfg: UiConfig = toml::from_str(toml_src).unwrap();
    assert_eq!(
        cfg.extra.get("keepawake_display"),
        Some(&toml::Value::Boolean(true))
    );
    let serialized = toml::to_string(&cfg).unwrap();
    assert!(
        serialized.contains("keepawake_display = true"),
        "extra persists to disk: {serialized}"
    );
}

#[test]
fn memory_l1_budget_default_is_300() {
    let cfg: MemoryConfig = toml::from_str("").unwrap();
    assert_eq!(cfg.l1_budget_tokens, 300);
}

#[test]
fn memory_l1_budget_user_override() {
    let cfg: MemoryConfig = toml::from_str("l1_budget_tokens = 1200").unwrap();
    assert_eq!(cfg.l1_budget_tokens, 1200);
}

#[test]
fn orchestration_plan_reminder_defaults() {
    let cfg: OrchestrationConfig = toml::from_str("").unwrap();
    assert!(cfg.plan_reminders);
    assert_eq!(cfg.nudge_interval_secs, 1800);
    assert_eq!(cfg.reflect.every_gates, 5);
    assert!(cfg.digests.enabled);
    assert_eq!(cfg.digests.rules.len(), 2);

    let serialized = toml::to_string(&cfg).unwrap();
    assert!(!serialized.contains(concat!("nudge_after_", "turns")));
    assert!(!serialized.contains(concat!("idle_", "poke")));
}

#[test]
fn usage_config_defaults_on() {
    let cfg: UsageConfig = toml::from_str("").unwrap();
    assert!(cfg.enabled);
    assert!(cfg.heartbeat);
    assert_eq!(cfg.freshness_secs, 180);
    assert_eq!(cfg.standalone_cadence_secs, 0);
}

#[test]
fn legacy_usage_threshold_is_ignored() {
    let cfg: UsageConfig = toml::from_str("high_pct_threshold = 75").unwrap();
    assert!(cfg.enabled);
}

#[test]
fn orchestration_reflect_every_gates_override() {
    let cfg: OrchestrationConfig = toml::from_str("[reflect]\nevery_gates = 25\n").unwrap();
    assert_eq!(cfg.reflect.every_gates, 25);
}

#[test]
fn orchestration_refresh_defaults_after_reflection_off() {
    let cfg: OrchestrationConfig = toml::from_str("").unwrap();
    assert!(
        !cfg.refresh.after_reflection,
        "after_reflection is OFF by default"
    );
    assert_eq!(cfg.refresh.handoff_timeout_secs, 60);
    assert_eq!(cfg.refresh.settle_timeout_secs, 120);
    assert_eq!(cfg.refresh.reorient_timeout_secs, 30);
    assert_eq!(cfg.refresh.verify_timeout_secs, 180);
}

#[test]
fn orchestration_refresh_overrides() {
    let cfg: OrchestrationConfig = toml::from_str(
        "[refresh]\nafter_reflection = true\nsettle_timeout_secs = 5\nverify_timeout_secs = 20\n",
    )
    .unwrap();
    assert!(cfg.refresh.after_reflection);
    assert_eq!(cfg.refresh.settle_timeout_secs, 5);
    assert_eq!(cfg.refresh.verify_timeout_secs, 20);
    assert_eq!(cfg.refresh.handoff_timeout_secs, 60);
}

#[test]
fn orchestration_plan_reminder_overrides() {
    let cfg: OrchestrationConfig =
        toml::from_str("plan_reminders = false\nnudge_interval_secs = 90").unwrap();
    assert!(!cfg.plan_reminders);
    assert_eq!(cfg.nudge_interval_secs, 90);
}

#[test]
fn telegram_allowed_users_accepts_quoted_strings() {
    let toml = r#"
        telegram_enabled = true
        telegram_allowed_users = ["100000001"]
    "#;
    let cfg: IntegrationsConfig = toml::from_str(toml).unwrap();
    assert_eq!(cfg.telegram_allowed_users, vec!["100000001".to_string()]);
}

#[test]
fn telegram_allowed_users_accepts_bare_integers() {
    let toml = r#"
        telegram_enabled = true
        telegram_allowed_users = [100000001]
    "#;
    let cfg: IntegrationsConfig = toml::from_str(toml).unwrap();
    assert_eq!(
        cfg.telegram_allowed_users,
        vec!["100000001".to_string()],
        "bare integer chat ids must coerce to strings — was silent rejection"
    );
}

#[test]
fn telegram_allowed_users_accepts_mixed() {
    let toml = r#"
        telegram_allowed_users = [100000001, "789012345"]
    "#;
    let cfg: IntegrationsConfig = toml::from_str(toml).unwrap();
    assert_eq!(
        cfg.telegram_allowed_users,
        vec!["100000001".to_string(), "789012345".to_string()]
    );
}

#[test]
fn discord_allowed_users_also_tolerant() {
    let toml = "discord_allowed_users = [555, \"666\"]";
    let cfg: IntegrationsConfig = toml::from_str(toml).unwrap();
    assert_eq!(
        cfg.discord_allowed_users,
        vec!["555".to_string(), "666".to_string()]
    );
}

/// Absent config is the whole default contract: an install that never opened
/// the panel keeps every record and interrupts nowhere, at every level.
#[test]
fn notification_settings_absent_config_interrupts_nowhere() {
    let empty: NotificationConfig = toml::from_str("").unwrap();
    let default = NotificationConfig::default();

    assert!(!empty.desktop, "the desktop surface is opt-in");
    assert!(!empty.sound);
    assert_eq!(empty.toast_duration, 5);
    assert!(
        empty.show_bell,
        "the surface the records land on is still drawn"
    );
    assert!(empty.channels.is_empty(), "push stays opt-in");

    assert_eq!(
        empty.agent_levels, default.agent_levels,
        "an absent grid is the generated grid"
    );
    for row in &empty.agent_levels {
        assert!(
            row.bell,
            "{} still carries the retained field, which routes nothing",
            row.level
        );
        for destination in fixture_columns() {
            if destination == NotifyDestination::Bell {
                continue;
            }
            assert!(
                !row.allows(destination),
                "{} -> {} is off until someone opts into it",
                row.level,
                destination.as_str()
            );
        }
    }

    // A row that names only its level named no column at all, so it reaches
    // nothing: not the pre-grid set of destinations, and not a default.
    let partial: NotificationConfig = toml::from_str(
        r#"
[[agent_levels]]
level = "verbose"
"#,
    )
    .unwrap();
    let verbose = &partial.agent_levels[0];
    assert!(!verbose.bell && !verbose.desktop);
    assert!(
        !verbose.allows(NotifyDestination::Channel(FIXTURE_CHANNEL)),
        "a row that never named a channel does not reach it"
    );
    assert!(verbose.reaches_nothing(&fixture_columns()));
}

/// The rows are `ALL` mapped. The proof is a fixture that gains a level: the
/// reconciled grid gains a row without anyone editing a list of three.
#[test]
fn notification_settings_rows_derive_from_the_shared_level_set() {
    let generated: Vec<String> = NotificationConfig::default()
        .agent_levels
        .into_iter()
        .map(|row| row.level)
        .collect();
    assert_eq!(
        generated,
        NotifyLevel::wire_values(),
        "the shipped grid is the shared set mapped, not a hand-written triple"
    );

    // Stored answers survive reconciliation; a level nobody answered arrives
    // interrupting nowhere.
    let mut only_failure =
        AgentLevelChoice::open(NotifyLevel::Failure.as_str(), &[FIXTURE_CHANNEL]);
    only_failure.desktop = false;
    let reconciled = reconcile_agent_levels(&[only_failure]);
    assert_eq!(
        reconciled.len(),
        NotifyLevel::ALL.len(),
        "every level of the shared set gets exactly one row"
    );
    let failure_row = reconciled
        .iter()
        .find(|row| row.level == NotifyLevel::Failure.as_str())
        .expect("the answered level keeps its row");
    assert!(!failure_row.desktop, "the stored answer is preserved");
    assert!(failure_row.bell);
    assert!(failure_row.allows(NotifyDestination::Channel(FIXTURE_CHANNEL)));
    for row in &reconciled {
        assert!(
            NotifyLevel::parse(&row.level).is_some(),
            "{} is not a level of the shared set",
            row.level
        );
    }

    // A row the set does not know is dropped rather than rendered.
    let retired = AgentLevelChoice::open("milestone", &[FIXTURE_CHANNEL]);
    let cleaned = reconcile_agent_levels(&[retired]);
    assert_eq!(cleaned, NotificationConfig::default().agent_levels);
    assert!(cleaned.iter().all(|row| row.level != "milestone"));
}

/// All-off is a configuration, not a validation error, and it round-trips.
#[test]
fn notification_settings_all_off_for_a_level_is_accepted_and_round_trips() {
    let mut cfg = NotificationConfig::default();
    let verbose = cfg
        .agent_levels
        .iter_mut()
        .find(|row| row.level == NotifyLevel::Verbose.as_str())
        .expect("verbose has a row");
    for destination in fixture_columns() {
        verbose.set(destination, false);
    }
    assert!(verbose.reaches_nothing(&fixture_columns()));

    let encoded = toml::to_string(&cfg).expect("an all-off level serialises");
    let decoded: NotificationConfig = toml::from_str(&encoded).expect("and parses back");
    let reloaded = decoded
        .agent_levels
        .iter()
        .find(|row| row.level == NotifyLevel::Verbose.as_str())
        .expect("the row survives the round trip");
    assert!(
        reloaded.reaches_nothing(&fixture_columns()),
        "a level that reaches nothing stays a level that reaches nothing"
    );
    assert_eq!(decoded.agent_levels, cfg.agent_levels);

    // Reconciliation must not quietly re-open what the user closed.
    assert_eq!(
        reconcile_agent_levels(&decoded.agent_levels),
        cfg.agent_levels
    );
}

/// The `bell` field is retained and inert, and retaining it is not sentiment:
/// `channels` is flattened, so a row that stopped naming the field would read a
/// stored `bell = true` back as a push channel called "bell" that no registry
/// will ever resolve. This pins both halves — the round trip, and the map
/// staying clean — so removing the field fails here rather than in someone's
/// config.
#[test]
fn notification_settings_the_retained_bell_field_never_becomes_a_channel_key() {
    let stored: NotificationConfig = toml::from_str(
        r#"
[[agent_levels]]
level = "failure"
bell = true
desktop = false
"#,
    )
    .expect("a stored row that names the field parses");
    let row = &stored.agent_levels[0];
    assert!(row.bell, "the field is read back as itself");
    assert!(
        row.channels.is_empty(),
        "and never lands in the flattened channel map: {:?}",
        row.channels
    );
    assert!(
        !row.allows(NotifyDestination::Channel("bell")),
        "so nothing resolves it as a channel either"
    );

    let encoded = toml::to_string(&stored).expect("it serialises");
    let decoded: NotificationConfig = toml::from_str(&encoded).expect("and parses back");
    assert_eq!(
        decoded.agent_levels, stored.agent_levels,
        "and round-trips unchanged"
    );

    // Inert, not absent. `show_bell` is the one bell answer config still makes,
    // it is a different field on a different struct, and moving it moves no row.
    let hidden = NotificationConfig {
        show_bell: false,
        ..NotificationConfig::default()
    };
    assert!(
        hidden.agent_levels.iter().all(|row| row.bell),
        "hiding the indicator routes nothing away from anything"
    );
    assert_ne!(
        toml::to_string(&hidden).unwrap(),
        toml::to_string(&NotificationConfig::default()).unwrap(),
        "and it is still a state config can express on its own"
    );
}

/// `allows`/`set` dispatch per column, so a column the roster gains cannot
/// leave another one silently unreadable.
#[test]
fn notification_settings_every_destination_is_independently_togglable() {
    let columns = NotifyDestination::columns(["fixture", "another-fixture"]);
    for target in &columns {
        let mut row = AgentLevelChoice::open(
            NotifyLevel::ActionNeeded.as_str(),
            &["fixture", "another-fixture"],
        );
        row.set(*target, false);
        for destination in &columns {
            assert_eq!(
                row.allows(*destination),
                destination != target,
                "toggling {} must not move {}",
                target.as_str(),
                destination.as_str()
            );
        }
        assert!(!row.reaches_nothing(&columns));
    }
}

/// THE DEFECT TEST, storage side. The chime used to key on a hardcoded table
/// that no configuration could contradict. Now every level answers for itself,
/// and the answer wins even where it disagrees with what shipped — which is the
/// whole point, so both directions of disagreement are exercised here.
#[test]
fn notification_level_sound_is_chosen_per_level_and_beats_the_shipped_rule() {
    for level in NotifyLevel::ALL {
        let unanswered = AgentLevelChoice::bell_only(level.as_str());
        assert_eq!(
            unanswered.sounds(),
            shipped_sound(level),
            "an unanswered {} falls back to what shipped",
            level.as_str()
        );

        // The inverse of the shipped answer, stated by the user. It must win.
        let chosen = !shipped_sound(level);
        let mut row = AgentLevelChoice::bell_only(level.as_str());
        row.sound = Some(chosen);
        assert_eq!(
            row.sounds(),
            chosen,
            "{} must honour the choice, not the table it disagrees with",
            level.as_str()
        );
    }

    // Concretely, so a table that quietly returned would still be caught: the
    // levels the old rule sounded are silenced, and the one it silenced sounds.
    let mut verbose = AgentLevelChoice::bell_only(NotifyLevel::Verbose.as_str());
    verbose.sound = Some(true);
    assert!(verbose.sounds(), "progress sounds when the user says so");
    for stopped in [NotifyLevel::ActionNeeded, NotifyLevel::Failure] {
        let mut row = AgentLevelChoice::bell_only(stopped.as_str());
        row.sound = Some(false);
        assert!(
            !row.sounds(),
            "{} stays silent when the user says so",
            stopped.as_str()
        );
    }

    // A level outside the set has no shipped answer to fall back to, so an
    // unanswered row for it is silent rather than guessed at.
    let unknown = AgentLevelChoice::bell_only("attention_soon");
    assert!(!unknown.sounds());
}

/// The stored shape. `channels` is flattened, so a `sound` key that was not a
/// NAMED field would deserialise into that map as a push channel called
/// "sound" — the exact bite the retained `bell` field exists to avoid. This
/// pins the round trip and the map staying clean, in both answers and unanswered.
#[test]
fn notification_level_sound_round_trips_and_never_lands_in_the_channel_map() {
    let stored: NotificationConfig = toml::from_str(
        r#"
sound = true

[[agent_levels]]
level = "verbose"
bell = true
desktop = false
sound = true

[[agent_levels]]
level = "action_needed"
bell = true
desktop = false
sound = false
fixture = true

[[agent_levels]]
level = "failure"
bell = true
desktop = false
"#,
    )
    .expect("rows that answer the chime parse");

    assert!(
        stored.sound,
        "the master mute is a field of its own section"
    );
    let rows = &stored.agent_levels;
    assert_eq!(
        rows[0].sound,
        Some(true),
        "an answer is read back as itself"
    );
    assert_eq!(rows[1].sound, Some(false), "including an answer of false");
    assert_eq!(
        rows[2].sound, None,
        "and an omitted answer stays unanswered"
    );

    for row in rows {
        assert!(
            !row.channels.contains_key("sound"),
            "no sound key reached the flattened channel map: {:?}",
            row.channels
        );
        assert!(
            !row.allows(NotifyDestination::Channel("sound")),
            "so nothing resolves the chime as a channel either"
        );
    }
    assert_eq!(
        rows[1].channels.get(FIXTURE_CHANNEL),
        Some(&true),
        "a real channel key still lands in the map beside the named field"
    );

    // The answers survive the write, the read, and the reconcile that runs on
    // every load — a level answered once must not be re-defaulted on restart.
    let encoded = toml::to_string(&stored).expect("it serialises");
    let decoded: NotificationConfig = toml::from_str(&encoded).expect("and parses back");
    assert_eq!(
        decoded.agent_levels, stored.agent_levels,
        "round-trips unchanged"
    );
    assert_eq!(
        reconcile_agent_levels(&decoded.agent_levels),
        stored.agent_levels,
        "and reconciliation keeps every answer, including the unanswered row"
    );

    // Unanswered is absent, not `false`: the default row writes no key at all,
    // which is what lets the shipped fallback stay reachable after a save.
    let fresh = toml::to_string(&NotificationConfig::default()).expect("the default serialises");
    let rows_only = &fresh[fresh.find("[[agent_levels]]").expect("rows are written")..];
    assert!(
        !rows_only.contains("sound"),
        "an unanswered row writes no sound key: {rows_only}"
    );
}
