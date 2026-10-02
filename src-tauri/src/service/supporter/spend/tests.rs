use super::*;

fn price() -> DecisionPricing {
    DecisionPricing {
        display_name: "priced fixture".into(),
        input_usd_per_million: 0.042,
        output_usd_per_million: 0.0,
    }
}

#[test]
fn supporter_spend_accumulates_usage_and_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let book = SpendBook::new(dir.path().into());
    let now = 1_735_689_600_000;
    book.set_balance("adapter", "fixture", Some(&price()), Some(5.0), now)
        .unwrap();
    let usage = DecisionUsage {
        input_tokens: 1_000_000,
        output_tokens: 400,
    };
    book.finish("adapter", "fixture", Some(usage), Some(&price()), None, now)
        .unwrap();
    book.finish(
        "adapter",
        "fixture",
        Some(usage),
        Some(&price()),
        None,
        now + 1,
    )
    .unwrap();
    drop(book);
    let book = SpendBook::new(dir.path().into());
    let status = book.status("adapter", now + 2);
    assert_eq!(status["today"]["input_tokens"], 2_000_000);
    assert_eq!(status["month"]["output_tokens"], 800);
    assert!((status["remaining_usd"].as_f64().unwrap() - 4.916).abs() < 1e-10);
    assert_eq!(status["since_balance"]["input_tokens"], 2_000_000);
    let next_day = book.status("adapter", now + 86_400_000);
    assert_eq!(next_day["today"]["input_tokens"], 0);
    assert_eq!(next_day["month"]["input_tokens"], 2_000_000);
    let next_month = book.status("adapter", now + 32 * 86_400_000);
    assert_eq!(next_month["month"]["input_tokens"], 0);
    assert_eq!(next_month["since_balance"]["input_tokens"], 2_000_000);
}

#[test]
fn supporter_low_balance_alert_fires_once_per_entry_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    let book = SpendBook::new(dir.path().into());
    assert!(book
        .set_balance("a", "fixture", Some(&price()), Some(1.01), 1)
        .unwrap()
        .is_empty());
    let usage = DecisionUsage {
        input_tokens: 1_000_000,
        output_tokens: 0,
    };
    let alerts = book
        .finish("a", "fixture", Some(usage), Some(&price()), None, 2)
        .unwrap();
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].level, NotifyLevel::ActionNeeded);
    assert!(book
        .finish("a", "fixture", Some(usage), Some(&price()), None, 3)
        .unwrap()
        .is_empty());
    drop(book);
    let book = SpendBook::new(dir.path().into());
    assert!(book
        .finish("a", "fixture", Some(usage), Some(&price()), None, 4)
        .unwrap()
        .is_empty());
    let next = book
        .set_balance("a", "fixture", Some(&price()), Some(0.5), 5)
        .unwrap();
    assert_eq!(next.len(), 1);
    assert_ne!(next[0].event_key, alerts[0].event_key);
    assert_eq!(book.status("a", 5)["since_balance"]["input_tokens"], 0);
}

#[test]
fn supporter_credit_exhaustion_blocks_and_reprobes_only_on_hourly_request() {
    let dir = tempfile::tempdir().unwrap();
    let book = SpendBook::new(dir.path().into());
    let error = DecisionError::Http(402);
    let alerts = book
        .finish("a", "fixture", None, Some(&price()), Some(&error), 1000)
        .unwrap();
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].level, NotifyLevel::Failure);
    assert_eq!(book.admit("a", 1001), Err(DecisionError::CreditExhausted));
    assert_eq!(
        book.admit("a", 3_600_999),
        Err(DecisionError::CreditExhausted)
    );
    assert!(book.admit("a", 3_601_000).is_ok());
    assert_eq!(
        book.admit("a", 3_601_001),
        Err(DecisionError::CreditExhausted)
    );
    assert!(book
        .finish(
            "a",
            "fixture",
            None,
            Some(&price()),
            Some(&error),
            3_601_002
        )
        .unwrap()
        .is_empty());
    drop(book);
    let book = SpendBook::new(dir.path().into());
    assert_eq!(
        book.admit("a", 3_601_003),
        Err(DecisionError::CreditExhausted)
    );
    book.set_balance("a", "fixture", Some(&price()), Some(10.0), 3_601_004)
        .unwrap();
    assert!(book.admit("a", 3_601_005).is_ok());
    assert_eq!(
        book.finish(
            "a",
            "fixture",
            None,
            Some(&price()),
            Some(&error),
            3_601_006
        )
        .unwrap()
        .len(),
        1
    );
    book.finish("a", "fixture", None, Some(&price()), None, 3_601_007)
        .unwrap();
    assert!(book.admit("a", 3_601_008).is_ok());
    assert_eq!(
        book.finish(
            "a",
            "fixture",
            None,
            Some(&price()),
            Some(&error),
            3_601_009
        )
        .unwrap()
        .len(),
        1
    );
}

#[test]
fn supporter_unpriced_adapters_report_tokens_and_local_zero_price_is_free() {
    let dir = tempfile::tempdir().unwrap();
    let book = SpendBook::new(dir.path().into());
    let usage = DecisionUsage {
        input_tokens: 500,
        output_tokens: 100,
    };
    assert!(book
        .set_balance("unknown", "fixture", None, Some(0.5), 999)
        .unwrap()
        .is_empty());
    assert_eq!(book.status("unknown", 999)["remaining_usd"], Value::Null);
    book.finish("unknown", "fixture", Some(usage), None, None, 1000)
        .unwrap();
    let status = book.status("unknown", 1000);
    assert_eq!(status["today"]["input_tokens"], 500);
    assert_eq!(status["today"]["estimated_usd"], Value::Null);
    let free = DecisionPricing {
        display_name: "local fixture".into(),
        input_usd_per_million: 0.0,
        output_usd_per_million: 0.0,
    };
    book.finish("free", "fixture", Some(usage), Some(&free), None, 1000)
        .unwrap();
    assert_eq!(book.status("free", 1000)["today"]["estimated_usd"], 0.0);
    book.finish("missing", "fixture", None, Some(&price()), None, 1000)
        .unwrap();
    assert_eq!(
        book.status("missing", 1000)["today"]["estimated_usd"],
        Value::Null
    );
}
