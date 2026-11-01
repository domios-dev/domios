use super::*;
#[test]
fn own_send_counts_advance_locally_and_reset_at_the_utc_day_boundary() {
    let mut counts = Counts::default();
    counts.record("first", 100).unwrap();
    counts.record("first", 101).unwrap();
    counts.record("second", 102).unwrap();
    let mut list = json!({"friends":[{"friendship_id":"first"},{"friendship_id":"second"}]});
    counts.project(&mut list, 103);
    assert_eq!(list["friends"][0]["sent_today"], 2);
    assert_eq!(list["friends"][1]["sent_today"], 1);
    counts.project(&mut list, 86400);
    assert_eq!(list["friends"][0]["sent_today"], 0);
    counts.record("first", 86401).unwrap();
    counts.project(&mut list, 86402);
    assert_eq!(list["friends"][0]["sent_today"], 1);
    assert_eq!(list["friends"][1]["sent_today"], 0);
}
