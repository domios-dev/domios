use super::*;

#[tokio::test]
async fn malformed_attachment_body_never_reaches_gateway_or_ack() {
    let f = Fixture::new();
    let keys = recipient();
    let mut message = message();
    let v = vector();
    let sender = crate::crypto::tests::keys(
        v["senderSeed"].as_str().unwrap(),
        v["seed"].as_str().unwrap(),
    );
    sender
        .encrypt(
            &mut message.doc.envelope,
            v["recipientSpki"].as_str().unwrap(),
            r#"{"attachments":[{"key_b64u":"private-key"}]}"#,
            &mut rand::rngs::OsRng,
        )
        .unwrap();
    message.doc.signature = sender.sign(message.doc.envelope.clone()).unwrap().signature;
    assert_eq!(
        receiver(&f, &keys, &message)
            .process(&message, NOW)
            .await
            .unwrap(),
        ReceiveOutcome::Rejected(Error::InvalidEnvelope)
    );
    assert_eq!(*f.events.lock().unwrap(), ["verify", "cursor"]);
    assert_eq!(*f.writes.lock().unwrap(), 0);
}
