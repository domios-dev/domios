use super::*;

const MECHANISM_SOURCE: &str = include_str!("../dedupe.rs");

fn tab_provenance(tab_id: &str) -> NotificationProvenance {
    NotificationProvenance {
        tab_id: Some(tab_id.to_string()),
        tab_title: Some("Worker lane".into()),
        workspace_id: Some("ws_lanes".into()),
        peer_name: None,
    }
}

/// The mechanism's code without its prose, so the clock scan below reads what
/// the module does rather than what it says about itself.
fn mechanism_code() -> String {
    MECHANISM_SOURCE
        .lines()
        .map(|line| line.split("//").next().unwrap_or_default())
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn identity(
    provenance: &NotificationProvenance,
    title: &str,
    body: Option<&str>,
) -> NotificationIdentity {
    NotificationIdentity::of(provenance, &NotificationLevel::Info, title, body)
}

#[test]
fn notify_dedupe_identity_round_trips_the_values_it_was_built_from() {
    let provenance = tab_provenance("tab_7f21c0de");
    let built = NotificationIdentity::of(
        &provenance,
        &NotificationLevel::Warning,
        "  build finished  ",
        Some("  release artifacts ready  "),
    );

    assert_eq!(built.scope(), "tab:tab_7f21c0de");
    assert_eq!(built.level(), &NotificationLevel::Warning);
    assert_eq!(built.title(), "build finished");
    assert_eq!(built.body(), "release artifacts ready");
    assert_eq!(
        built,
        NotificationIdentity::of(
            &provenance,
            &NotificationLevel::Warning,
            "build finished",
            Some("release artifacts ready"),
        ),
        "identity is the values, not the byte-for-byte payload"
    );
}

#[test]
fn notify_dedupe_same_message_reaches_the_sink_once() {
    let provenance = tab_provenance("tab_a");
    let mut ledger = DeliveryLedger::default();
    let key = identity(&provenance, "agent needs input", Some("approve the step?"));

    assert_eq!(ledger.existing(&key), None);
    let first = DeliveryAdmission::Deliver("notice-1".into());
    ledger.bind(key.clone(), first.id().to_string());

    let second = match ledger.existing(&key) {
        Some(id) => DeliveryAdmission::Duplicate(id),
        None => panic!("an identical live message must be recognised"),
    };

    assert!(first.reaches_sink(), "the first arrival is delivered");
    assert!(!second.reaches_sink(), "одного достаточно: the second is not");
    assert_eq!(
        second.id(),
        first.id(),
        "the duplicate names the record that already exists"
    );
    assert_eq!(ledger.live_len(), 1, "no second record is opened");
}

#[test]
fn notify_dedupe_distinct_messages_both_arrive() {
    let provenance = tab_provenance("tab_a");
    let mut ledger = DeliveryLedger::default();

    let cases = [
        ("agent needs input", Some("approve the step?")),
        ("agent needs input", Some("approve a different step?")),
        ("build failed", Some("approve the step?")),
        ("agent needs input", None),
    ];

    for (index, &(title, body)) in cases.iter().enumerate() {
        let key = identity(&provenance, title, body);
        assert_eq!(
            ledger.existing(&key),
            None,
            "{title:?}/{body:?} is a genuinely different message and must arrive"
        );
        ledger.bind(key, format!("notice-{index}"));
    }

    assert_eq!(ledger.live_len(), cases.len());

    let same_words_other_origin = identity(&tab_provenance("tab_b"), "build failed", None);
    assert_eq!(
        ledger.existing(&same_words_other_origin),
        None,
        "the same words from another origin are another message"
    );
}

#[test]
fn notify_dedupe_release_re_arms_an_acknowledged_identity() {
    let provenance = tab_provenance("tab_a");
    let mut ledger = DeliveryLedger::default();
    let key = identity(&provenance, "agent needs input", None);
    ledger.bind(key.clone(), "notice-1".into());

    ledger.release("notice-1");
    assert_eq!(
        ledger.existing(&key),
        None,
        "a read or cleared record stops suppressing its own recurrence"
    );

    ledger.bind(key.clone(), "notice-2".into());
    ledger.release_scope("tab:tab_a");
    assert_eq!(ledger.existing(&key), None, "a closing scope releases too");
    assert_eq!(ledger.live_len(), 0);
}

#[test]
fn notify_dedupe_suppression_is_keyed_on_identity_and_never_on_elapsed_time() {
    let provenance = tab_provenance("tab_a");
    let mut ledger = DeliveryLedger::default();
    let key = identity(&provenance, "codex available", None);
    ledger.bind(key.clone(), "notice-1".into());

    for other in 0..1_000 {
        ledger.bind(
            identity(&provenance, &format!("unrelated {other}"), None),
            format!("filler-{other}"),
        );
    }

    assert_eq!(
        ledger.existing(&key).as_deref(),
        Some("notice-1"),
        "no window expires the suppression: only an explicit release does"
    );

    // Every timer form has to name a clock type or a delay to be written at
    // all, so the mechanism keeping clear of them is checkable by scan.
    let mechanism = mechanism_code();
    for clock in [
        "time", "instant", "elapsed", "interval", "duration", "delay", "debounce",
    ] {
        assert!(
            !mechanism.contains(clock),
            "the suppression mechanism must not reference {clock}"
        );
    }
}

#[test]
fn notify_dedupe_standing_condition_announces_on_transition_not_on_re_observation() {
    let mut ledger = ConditionLedger::default();
    let scope = "tab:tab_a";
    let condition = provider_update_condition("codex");

    assert_eq!(
        ledger.observe_present(scope, &condition),
        ConditionTransition::Announce,
        "becoming available is the news"
    );
    for _ in 0..50 {
        assert_eq!(
            ledger.observe_present(scope, &condition),
            ConditionTransition::AlreadyAnnounced,
            "still being available is not news, however often it is observed"
        );
    }

    ledger.observe_absent(scope, &condition);
    assert_eq!(
        ledger.observe_present(scope, &condition),
        ConditionTransition::Announce,
        "a resolved condition that becomes true again is a new transition"
    );
}

#[test]
fn notify_dedupe_standing_conditions_are_scoped_and_independent() {
    let mut ledger = ConditionLedger::default();
    let codex = provider_update_condition("codex");
    let claude = provider_update_condition("claude");

    assert_eq!(
        ledger.observe_present("tab:tab_a", &codex),
        ConditionTransition::Announce
    );
    assert_eq!(
        ledger.observe_present("tab:tab_b", &codex),
        ConditionTransition::Announce,
        "another origin has not been told yet"
    );
    assert_eq!(
        ledger.observe_present("tab:tab_a", &claude),
        ConditionTransition::Announce,
        "another condition is another transition"
    );
    assert_eq!(
        ledger.observe_present("tab:tab_a", &codex),
        ConditionTransition::AlreadyAnnounced
    );

    ledger.close_scope("tab:tab_a");
    assert_eq!(
        ledger.observe_present("tab:tab_a", &codex),
        ConditionTransition::Announce,
        "a closed scope releases only its own latches"
    );
    assert_eq!(
        ledger.observe_present("tab:tab_b", &codex),
        ConditionTransition::AlreadyAnnounced,
        "an unrelated scope keeps its latch"
    );
}
