use super::*;

fn price() -> DecisionPricing {
    DecisionPricing {
        display_name: "priced fixture".into(),
        input_usd_per_million: 0.042,
        output_usd_per_million: 0.0,
    }
}

#[test]
fn supporter_spend_accumulates_today_and_month_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    let book = SpendBook::new(dir.path().into());
    let now = 1_735_689_600_000;
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
    assert_eq!(status["today"]["estimated_usd"], 0.084);
    let next_day = book.status("adapter", now + 86_400_000);
    assert_eq!(next_day["today"]["input_tokens"], 0);
    assert_eq!(next_day["month"]["input_tokens"], 2_000_000);
    let next_month = book.status("adapter", now + 32 * 86_400_000);
    assert_eq!(next_month["month"]["input_tokens"], 0);
}

#[test]
fn supporter_credit_exhaustion_alerts_once_blocks_and_rearms_after_success() {
    let dir = tempfile::tempdir().unwrap();
    let book = SpendBook::new(dir.path().into());
    let error = DecisionError::Http(402);
    let alerts = book
        .finish("a", "fixture", None, Some(&price()), Some(&error), 1_000)
        .unwrap();
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].level, NotifyLevel::Failure);
    assert!(alerts[0].title.contains("out of credit"));
    assert_eq!(book.admit("a", 1_001), Err(DecisionError::CreditExhausted));
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
    assert!(book.admit("a", 7_201_002).is_ok());
    book.finish("a", "fixture", None, Some(&price()), None, 7_201_003)
        .unwrap();
    assert_eq!(
        book.finish(
            "a",
            "fixture",
            None,
            Some(&price()),
            Some(&error),
            7_201_004
        )
        .unwrap()
        .len(),
        1
    );
    book.finish("a", "fixture", None, Some(&price()), None, 7_201_005)
        .unwrap();
    assert!(book.admit("a", 7_201_006).is_ok());
}

#[test]
fn supporter_unpriced_adapters_report_tokens_and_local_zero_price_is_free() {
    let dir = tempfile::tempdir().unwrap();
    let book = SpendBook::new(dir.path().into());
    let usage = DecisionUsage {
        input_tokens: 500,
        output_tokens: 100,
    };
    book.finish("unknown", "fixture", Some(usage), None, None, 1_000)
        .unwrap();
    let status = book.status("unknown", 1_000);
    assert_eq!(status["today"]["input_tokens"], 500);
    assert_eq!(status["today"]["estimated_usd"], Value::Null);
    let free = DecisionPricing {
        display_name: "local fixture".into(),
        input_usd_per_million: 0.0,
        output_usd_per_million: 0.0,
    };
    book.finish("free", "fixture", Some(usage), Some(&free), None, 1_000)
        .unwrap();
    assert_eq!(book.status("free", 1_000)["today"]["estimated_usd"], 0.0);
    book.finish("missing", "fixture", None, Some(&price()), None, 1_000)
        .unwrap();
    assert_eq!(
        book.status("missing", 1_000)["today"]["estimated_usd"],
        Value::Null
    );
}
