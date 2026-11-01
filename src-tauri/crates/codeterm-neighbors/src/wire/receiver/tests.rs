use super::*;

fn peers() -> PeerKeys {
    let key = |id: &str, created_at| PeerKey {
        verifier_key_id: format!("vk_{id}"),
        public_key_spki_b64: id.into(),
        encryption_public_key_spki_b64: id.into(),
        enc_key_id: format!("ek_{id}"),
        created_at,
    };
    PeerKeys {
        friendship_id: "a".repeat(64),
        peer_uid: "peer".into(),
        keys: vec![key("friend_target", 1), key("another_machine", 99)],
        peer_receiver: Some(PeerReceiver {
            enc_key_id: Some("ek_friend_target".into()),
            revision: 9,
            receiving: true,
        }),
        my_receiver: None,
    }
}

#[test]
fn sealing_uses_the_effective_friend_key_instead_of_the_newest_account_key() {
    let mut peers = peers();
    assert_eq!(peers.receiver_key().unwrap().enc_key_id, "ek_friend_target");
    peers.peer_receiver = None;
    assert!(matches!(
        peers.receiver_key(),
        Err(Error::RecipientNotReceiving)
    ));
}

#[test]
fn disabled_missing_or_malformed_effective_authority_never_falls_back() {
    for (receiving, key, revision) in [
        (false, Some("ek_friend_target"), 9),
        (true, None, 9),
        (true, Some("ek_missing"), 9),
        (true, Some("ek_friend_target"), MAX_SAFE_INTEGER + 1),
    ] {
        let mut peers = peers();
        peers.peer_receiver = Some(PeerReceiver {
            receiving,
            enc_key_id: key.map(str::to_owned),
            revision,
        });
        assert!(matches!(
            peers.receiver_key(),
            Err(Error::RecipientNotReceiving)
        ));
    }
}

#[test]
fn an_old_sender_is_refused_before_text_sealing_or_file_upload() {
    let mut peers = peers();
    peers.my_receiver = Some(EffectiveReceiver {
        installation_id: Some("new".into()),
        enc_key_id: Some("ek_new".into()),
        machine_label: Some("New desk".into()),
        revision: 2,
        receiving: true,
    });
    assert_eq!(
        peers.require_sender_receiver("old"),
        Err(Error::NotReceiver)
    );
    assert!(peers.require_sender_receiver("new").is_ok());
    peers.my_receiver.as_mut().unwrap().enc_key_id = None;
    assert_eq!(
        peers.require_sender_receiver("new"),
        Err(Error::InvalidEnvelope)
    );
}
