use super::*;
use std::collections::HashSet;

#[test]
fn short_id_is_prefixed_and_bounded() {
    let id = short_id("task");
    let (prefix, slug) = id.split_once('_').expect("prefixed");
    assert_eq!(prefix, "task");
    assert_eq!(slug.len(), 6);
    assert!(slug
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() && !b.is_ascii_uppercase()));
}

#[test]
fn mint_id_avoids_collisions_and_grows_on_saturation() {
    // Everything at length 6 is "taken" -> minter must escalate to a longer slug.
    let id = mint_id("task", |c| {
        c.split_once('_')
            .map(|(_, s)| s.len() == 6)
            .unwrap_or(false)
    });
    let slug = id.split_once('_').unwrap().1;
    assert!(
        slug.len() >= 7,
        "expected escalation past 6 chars, got {id}"
    );
}

#[test]
fn mint_id_returns_free_candidate() {
    let mut seen = HashSet::new();
    for _ in 0..2000 {
        let id = mint_id("sec", |c| seen.contains(c));
        assert!(seen.insert(id.clone()), "minted duplicate {id}");
    }
}

#[test]
fn fallible_collision_probe_stops_on_the_first_storage_error() {
    let mut probes = 0;
    let result = try_mint_id("task", |_| {
        probes += 1;
        Err::<bool, _>("unavailable")
    });
    assert_eq!(result, Err("unavailable"));
    assert_eq!(probes, 1);
}
