use super::*;

#[test]
fn pool_is_bounded_and_decisiveness_requires_a_clear_gap() {
    assert_eq!(
        selection(&(0..33).map(|i| i as f64).collect::<Vec<_>>()),
        (23..33).rev().collect::<Vec<_>>()
    );
    assert_eq!(skip_reason(&[0.2, 0.2, 0.2]), Some("small_pool"));
    assert_eq!(skip_reason(&[0.9, 0.3, 0.2, 0.1]), Some("decisive_ranking"));
    assert_eq!(skip_reason(&[0.9, 0.8, 0.2, 0.1]), None);
    assert_eq!(skip_reason(&[0.0; 4]), None);
    assert_eq!(
        skip_reason(&[-0.1, -0.7, -0.8, -0.9]),
        Some("decisive_ranking")
    );
    assert_eq!(skip_reason(&[-0.1, -0.2, -0.3, -0.4]), None);
    assert_eq!(skip_reason(&[f64::NAN, 0.1, 0.2, 0.3]), None);
}
