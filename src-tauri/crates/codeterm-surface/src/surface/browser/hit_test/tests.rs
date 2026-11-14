use super::*;

#[test]
fn browser_pointer_hit_test_fails_closed() {
    assert_eq!(
        hit_test_result(&json!({"result":{"value":"clear"}})),
        Ok(())
    );
    for reason in [
        "target_occluded: covered by button#cover",
        "stale: detached pointer target",
    ] {
        assert_eq!(
            hit_test_result(&json!({"result":{"value":reason}})),
            Err(reason.into())
        );
    }
    for reply in [
        json!({}),
        json!({"result":{"value":true}}),
        json!({"exceptionDetails":{}}),
    ] {
        assert_eq!(
            hit_test_result(&reply),
            Err("target_occluded: pointer hit-test unavailable".into())
        );
    }
}
