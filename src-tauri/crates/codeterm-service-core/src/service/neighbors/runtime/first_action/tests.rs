use super::*;

fn account(installation: Option<&str>) -> ReceiverStatus {
    ReceiverStatus {
        receiver_installation_id: installation.map(str::to_owned),
        receiver_key_id: None,
        receiver_machine_label: installation.map(str::to_owned),
        receiver_revision: 7,
        receiving_enabled: installation.is_some(),
        is_me: installation == Some("here"),
        legacy_active: false,
    }
}

fn list(installation: Option<&str>) -> Value {
    json!({"friends":[{"friendship_id":"friend", "my_receiver":{
        "source":"friend","installation_id":installation,"key_id":null,
        "enc_key_id":null,"session_id":null,"machine_label":installation,
        "revision":9,"features":[],"receiving":false
    }}]})
}

#[test]
fn accepting_and_granting_an_unassigned_friend_claims_the_observed_revision() {
    assert_eq!(
        first_claim_revision(&list(None), "friend", &account(None), "here"),
        Ok(Some(9))
    );
    assert_eq!(
        first_claim_revision(
            &json!({"friends":[{"friendship_id":"friend"}]}),
            "friend",
            &account(None),
            "here"
        ),
        Ok(Some(7))
    );
}

#[test]
fn any_assigned_receiver_prevents_automatic_claim_even_when_disabled() {
    assert_eq!(
        first_claim_revision(&list(Some("elsewhere")), "friend", &account(None), "here"),
        Ok(None)
    );
    assert_eq!(
        first_claim_revision(&list(None), "friend", &account(Some("elsewhere")), "here"),
        Ok(None)
    );
    let mut friends = list(None);
    let mut other = list(Some("elsewhere"))["friends"][0].clone();
    other["friendship_id"] = json!("other");
    friends["friends"].as_array_mut().unwrap().push(other);
    assert_eq!(
        first_claim_revision(&friends, "friend", &account(None), "here"),
        Ok(None)
    );
}

#[test]
fn missing_friend_and_malformed_projections_never_claim() {
    assert_eq!(
        first_claim_revision(&list(None), "foreign", &account(None), "here"),
        Err(Error::FriendshipEnded)
    );
    assert_eq!(
        first_claim_revision(
            &json!({"friends":[{"friendship_id":"friend","my_receiver":{}}]}),
            "friend",
            &account(None),
            "here"
        ),
        Err(Error::InvalidEnvelope)
    );
}

#[test]
fn refusals_distinguish_none_chosen_and_name_the_receiving_machine() {
    assert!(receiver_refusal("friend", "Anya", &account(None)).starts_with("receiver_not_chosen:"));
    let refusal = receiver_refusal("friend", "Anya", &account(Some("Anya's Mac")));
    assert!(refusal.contains("Messages from Anya are received on Anya's Mac. Receive them here?"));
    assert!(refusal.contains("--expected-receiver-revision 7 --takeover-confirmed"));
}
