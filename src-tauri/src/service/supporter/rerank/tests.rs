use super::*;

#[test]
fn pool_keeps_lowest_first_stage_scores_and_stable_ties() {
    assert_eq!(
        selection(&(0..33).rev().map(|i| i as f64).collect::<Vec<_>>()),
        (23..33).rev().collect::<Vec<_>>()
    );
    assert_eq!(selection(&[-0.1, -0.9, -0.9, 0.5]), vec![1, 2, 0, 3]);
}

#[test]
fn decisiveness_requires_a_clear_gap_after_the_best_hit() {
    assert_eq!(skip_reason(&[0.2, 0.2, 0.2]), Some("small_pool"));
    assert_eq!(skip_reason(&[0.1, 0.7, 0.8, 0.9]), Some("decisive_ranking"));
    assert_eq!(skip_reason(&[0.1, 0.2, 0.3, 0.9]), None);
    assert_eq!(skip_reason(&[0.0; 4]), None);
    assert_eq!(skip_reason(&[0.0, 0.0, 0.0, 0.0, 0.5]), None);
    assert_eq!(
        skip_reason(&[-0.9, -0.3, -0.2, -0.1]),
        Some("decisive_ranking")
    );
    assert_eq!(skip_reason(&[-0.9, -0.8, -0.2, -0.1]), None);
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(skip_reason(&[invalid, 0.1, 0.7, 0.8]), None);
    }
}
