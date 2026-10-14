use super::*;

#[test]
fn consumed_owner_attachments_confirm_the_complete_payload_only_in_the_own_session() {
    let records = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../plugins/provider-claude/tests/fixtures/owner-mid-turn-2.1.296.jsonl"
    ));
    for record in records.lines().filter_map(|line| {
        let value: serde_json::Value = serde_json::from_str(line).unwrap();
        (value["type"] == "attachment").then_some(value)
    }) {
        let ledger = ChatSendLedger::default();
        let prompt = record["attachment"]["prompt"].as_str().unwrap();
        let payload = prompt
            .split_once("<domios")
            .map_or(prompt, |(owner, _)| owner);
        let session = record["sessionId"].as_str().unwrap();
        let send = ledger
            .try_start_scoped(
                "tab",
                payload,
                None,
                Some(SendLiveBinding {
                    session_id: Some(session.into()),
                    binding_nonce: Some("launch".into()),
                }),
            )
            .unwrap();
        ledger.finish(&send.send_id, Ok(())).unwrap();
        for (source, nonce, text) in [
            ("foreign", Some("launch"), prompt),
            (session, Some("old-launch"), prompt),
            (session, Some("launch"), &payload[..payload.len() - 2]),
        ] {
            assert!(ledger
                .confirm_delivered_user_turns_scoped("tab", source, nonce, text)
                .is_empty());
        }
        let confirmed =
            ledger.confirm_delivered_user_turns_scoped("tab", session, Some("launch"), prompt);
        assert_eq!(confirmed.len(), 1);
        assert_eq!(confirmed[0].send_id, send.send_id);
        assert_eq!(confirmed[0].state, ChatSendState::Sent);
        assert_eq!(
            ledger
                .finish(&send.send_id, Err("submit_unconfirmed: late".into()))
                .unwrap()
                .state,
            ChatSendState::Sent
        );
    }
}
