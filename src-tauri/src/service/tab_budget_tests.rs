use super::*;

#[test]
fn under_soft_budget_yields_no_notice() {
    assert!(evaluate_tab_budget(0).is_none());
    assert!(evaluate_tab_budget(14).is_none());
    // Exactly at the soft budget is still fine — nudge fires only when it is EXCEEDED.
    assert!(evaluate_tab_budget(SOFT_TAB_BUDGET).is_none());
}

#[test]
fn just_over_soft_budget_yields_soft_notice_without_the_strong_sentence() {
    let notice = evaluate_tab_budget(SOFT_TAB_BUDGET + 1).expect("soft nudge");
    assert_eq!(notice.level, TabBudgetLevel::Soft);
    assert_eq!(notice.workspace_tab_count, 16);
    assert!(
        notice.message.contains("16 Tabs"),
        "names the count: {}",
        notice.message
    );
    assert!(
        !notice.message.contains("Are you sure"),
        "soft nudge must not use the strong wording: {}",
        notice.message
    );
}

#[test]
fn at_strong_budget_is_still_soft() {
    // 20 is NOT over 20 — strong fires strictly above STRONG_TAB_BUDGET.
    let notice = evaluate_tab_budget(STRONG_TAB_BUDGET).expect("soft nudge at 20");
    assert_eq!(notice.level, TabBudgetLevel::Soft);
    assert_eq!(notice.workspace_tab_count, 20);
}

#[test]
fn over_strong_budget_yields_strong_notice_with_exact_count() {
    let notice = evaluate_tab_budget(STRONG_TAB_BUDGET + 1).expect("strong nudge");
    assert_eq!(notice.level, TabBudgetLevel::Strong);
    assert_eq!(notice.workspace_tab_count, 21);
    assert!(
        notice.message.contains("21 Tabs"),
        "strong nudge names the exact count: {}",
        notice.message
    );
    assert!(
        notice
            .message
            .contains("Are you sure you did not forget to close finished Tabs"),
        "strong nudge uses the explicit wording: {}",
        notice.message
    );
}

#[test]
fn notice_serializes_level_as_snake_case() {
    let notice = evaluate_tab_budget(STRONG_TAB_BUDGET + 5).unwrap();
    let json = serde_json::to_value(&notice).unwrap();
    assert_eq!(json["level"], "strong");
    assert_eq!(json["workspace_tab_count"], 25);
    assert!(json["message"].is_string());
}
