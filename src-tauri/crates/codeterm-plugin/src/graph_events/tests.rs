use super::*;
use crate::js_runtime::manifest::EventSourceDecl;
use serde_json::json;

struct TestProvider;

impl EventSourceProvider for TestProvider {
    fn declarations(&self) -> &[EventSourceDecl] {
        &[]
    }

    fn subscribe(&self, _: &str, _: &Value, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }

    fn unsubscribe(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
}

struct CountingProvider(std::sync::atomic::AtomicUsize);

impl EventSourceProvider for CountingProvider {
    fn declarations(&self) -> &[EventSourceDecl] {
        &[]
    }

    fn subscribe(&self, _: &str, _: &Value, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }

    fn unsubscribe(&self, _: &str, _: &str) -> Result<(), String> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}

fn subscription<'a>(
    token: &'a str,
    plugin_id: &'a str,
    pending: usize,
    event_ids: &'a VecDeque<String>,
    admitted_at_ms: &'a VecDeque<u64>,
) -> AdmissionSubscription<'a> {
    AdmissionSubscription {
        token,
        plugin_id,
        payload_schema: None,
        pending,
        event_ids,
        admitted_at_ms,
    }
}

fn request<'a>(event_id: &'a str, payload: &'a Value, now_ms: u64) -> AdmissionRequest<'a> {
    AdmissionRequest {
        subscription_id: "sub",
        token: "secret",
        plugin_id: "plugin",
        event_id,
        payload,
        payload_bytes: serde_json::to_vec(payload).unwrap().len(),
        now_ms,
    }
}

#[test]
fn graph_events_admit_refusal_table_names_each_cause() {
    let ids = VecDeque::new();
    let times = VecDeque::new();
    let valid = subscription("secret", "plugin", 0, &ids, &times);
    let payload = json!({"text":"ok"});
    let mut rate_times = VecDeque::new();
    rate_times.extend(std::iter::repeat(20_000).take(MAX_EVENTS_PER_MINUTE));
    let rate_limited = subscription("secret", "plugin", 0, &ids, &rate_times);
    let duplicate_ids = VecDeque::from(["same".to_owned()]);
    let duplicate = subscription("secret", "plugin", 0, &duplicate_ids, &times);
    let full = subscription(
        "secret",
        "plugin",
        MAX_PENDING_PER_SUBSCRIPTION,
        &ids,
        &times,
    );
    let schema = crate::js_runtime::manifest::NodeSchema {
        properties: Vec::new(),
        required: vec!["value".into()],
    };
    let invalid = AdmissionSubscription {
        payload_schema: Some(&schema),
        ..subscription("secret", "plugin", 0, &ids, &times)
    };

    for (request, subscription, global, expected) in [
        (
            request("id", &payload, 1),
            None,
            0,
            AdmitRefusal::UnknownSubscription,
        ),
        (
            AdmissionRequest {
                token: "wrong",
                ..request("id", &payload, 1)
            },
            Some(&valid),
            0,
            AdmitRefusal::BadToken,
        ),
        (
            AdmissionRequest {
                plugin_id: "other",
                ..request("id", &payload, 1)
            },
            Some(&valid),
            0,
            AdmitRefusal::ForeignPlugin,
        ),
        (
            request("same", &payload, 1),
            Some(&duplicate),
            0,
            AdmitRefusal::Duplicate,
        ),
        (
            request("id", &payload, 1),
            Some(&invalid),
            0,
            AdmitRefusal::InvalidPayload,
        ),
        (
            request("id", &payload, 20_001),
            Some(&rate_limited),
            0,
            AdmitRefusal::RateLimited,
        ),
        (
            request("id", &payload, 1),
            Some(&full),
            0,
            AdmitRefusal::Backpressure,
        ),
        (
            request("id", &payload, 1),
            Some(&valid),
            MAX_PENDING_GLOBAL,
            AdmitRefusal::Backpressure,
        ),
        (
            AdmissionRequest {
                payload_bytes: MAX_EVENT_BYTES + 1,
                ..request("id", &payload, 1)
            },
            Some(&valid),
            0,
            AdmitRefusal::Oversize,
        ),
    ] {
        assert_eq!(
            admit(&request, subscription, global),
            AdmissionDecision::Refuse(expected)
        );
    }
}

#[test]
fn graph_events_subscription_queue_is_bounded_and_delivers_each_admission_once() {
    let table = SubscriptionTable::new();
    table
        .subscribe(
            "sub".into(),
            "pipeline".into(),
            "fixture.message".into(),
            "{}".into(),
            1,
            1,
            Arc::new(TestProvider),
            "secret".into(),
            "plugin".into(),
            None,
        )
        .unwrap();
    let payload = json!({"text":"hello"});
    for index in 0..MAX_PENDING_PER_SUBSCRIPTION {
        assert_eq!(
            table.admit(AdmissionRequest {
                subscription_id: "sub",
                token: "secret",
                plugin_id: "plugin",
                event_id: &index.to_string(),
                payload: &payload,
                payload_bytes: 17,
                now_ms: index as u64 * 60_001,
            }),
            AdmissionDecision::Admit
        );
    }
    assert_eq!(table.pending_len(), MAX_PENDING_PER_SUBSCRIPTION);
    assert_eq!(
        table.admit(AdmissionRequest {
            subscription_id: "sub",
            token: "secret",
            plugin_id: "plugin",
            event_id: "overflow",
            payload: &payload,
            payload_bytes: 17,
            now_ms: 65 * 60_001,
        }),
        AdmissionDecision::Refuse(AdmitRefusal::Backpressure)
    );
    for index in 0..MAX_PENDING_PER_SUBSCRIPTION {
        let next = table.next_for("pipeline").unwrap();
        assert_eq!(next.event_id, index.to_string());
        table.consume("sub", &next.event_id).unwrap();
        assert!(table.consume("sub", &next.event_id).is_none());
    }
    assert!(table.next_for("pipeline").is_none());
    assert_eq!(table.pending_len(), 0);
    let summary = table.summary("pipeline").unwrap();
    assert_eq!(summary.delivered, MAX_PENDING_PER_SUBSCRIPTION as u64);
    assert_eq!(summary.refused, 1);
}

#[test]
fn graph_events_global_queue_refuses_at_its_exact_bound() {
    let table = SubscriptionTable::new();
    for index in 0..69 {
        table
            .subscribe(
                format!("sub-{index}"),
                format!("pipeline-{index}"),
                "fixture.message".into(),
                "{}".into(),
                1,
                1,
                Arc::new(TestProvider),
                "secret".into(),
                "plugin".into(),
                None,
            )
            .unwrap();
    }
    let payload = json!({"message":"same"});
    for index in 0..MAX_PENDING_GLOBAL {
        let subscription_index = index / 60;
        let subscription_id = format!("sub-{subscription_index}");
        let event_id = index.to_string();
        assert_eq!(
            table.admit(AdmissionRequest {
                subscription_id: &subscription_id,
                token: "secret",
                plugin_id: "plugin",
                event_id: &event_id,
                payload: &payload,
                payload_bytes: 18,
                now_ms: index as u64,
            }),
            AdmissionDecision::Admit
        );
    }
    assert_eq!(table.pending_len(), MAX_PENDING_GLOBAL);
    assert_eq!(
        table.admit(AdmissionRequest {
            subscription_id: "sub-68",
            token: "secret",
            plugin_id: "plugin",
            event_id: "overflow",
            payload: &payload,
            payload_bytes: 18,
            now_ms: MAX_PENDING_GLOBAL as u64,
        }),
        AdmissionDecision::Refuse(AdmitRefusal::Backpressure)
    );
    assert_eq!(table.pending_len(), MAX_PENDING_GLOBAL);
}

#[test]
fn graph_events_unsubscribe_calls_provider_and_journals_queued_events() {
    let table = SubscriptionTable::new();
    let provider = Arc::new(CountingProvider(std::sync::atomic::AtomicUsize::new(0)));
    table
        .subscribe(
            "sub".into(),
            "pipeline".into(),
            "fixture.message".into(),
            "{}".into(),
            1,
            1,
            provider.clone(),
            "secret".into(),
            "plugin".into(),
            None,
        )
        .unwrap();
    let payload = json!({"message":"pending"});
    table.admit(AdmissionRequest {
        subscription_id: "sub",
        token: "secret",
        plugin_id: "plugin",
        event_id: "queued",
        payload: &payload,
        payload_bytes: 20,
        now_ms: 1,
    });

    assert!(table.unsubscribe("sub"));
    assert_eq!(provider.0.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(table.pending_len(), 0);
    assert_eq!(
        table.admission_journal()[0].outcome,
        JournalOutcome::Cancelled
    );
}

#[test]
fn graph_events_refusal_journal_marks_retention_overflow_and_keeps_new_refusal() {
    let table = SubscriptionTable::new();
    let payload = json!({});
    for index in 0..=MAX_REFUSAL_JOURNAL {
        let event_id = index.to_string();
        assert_eq!(
            table.admit(AdmissionRequest {
                subscription_id: "unknown",
                token: "secret",
                plugin_id: "plugin",
                event_id: &event_id,
                payload: &payload,
                payload_bytes: 2,
                now_ms: index as u64,
            }),
            AdmissionDecision::Refuse(AdmitRefusal::UnknownSubscription)
        );
    }
    let journal = table.admission_journal();
    assert_eq!(journal.len(), MAX_REFUSAL_JOURNAL);
    assert!(journal
        .iter()
        .any(|entry| entry.outcome == JournalOutcome::RetentionOverflow));
    assert_eq!(
        journal.last().unwrap().outcome,
        JournalOutcome::Refused(AdmitRefusal::UnknownSubscription)
    );
}
