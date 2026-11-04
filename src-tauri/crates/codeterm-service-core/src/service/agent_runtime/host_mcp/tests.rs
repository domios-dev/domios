use super::*;

#[test]
fn agent_runtime_host_mcp_absent_without_minted_local_credential_or_declaration() {
    for (declared, minted) in [(false, false), (false, true), (true, false)] {
        assert!(descriptor(
            declared,
            minted,
            "own",
            "nonce",
            Some(std::path::PathBuf::from("/trusted/domios"))
        )
        .is_none());
    }
    assert!(descriptor(true, true, "own", "nonce", None).is_none());
    let actual = descriptor(
        true,
        true,
        "own",
        "nonce",
        Some(std::path::PathBuf::from("/trusted/domios")),
    )
    .unwrap();
    assert_eq!(
        actual.descriptor(),
        serde_json::json!({"name":"domios","command":"/trusted/domios","args":["mcp","serve","--tab","own","--binding-nonce","nonce"],"env":[]})
    );
}
