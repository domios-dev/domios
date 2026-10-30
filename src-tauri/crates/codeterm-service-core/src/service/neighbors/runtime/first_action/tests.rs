use super::*;

fn list(installation: Option<&str>) -> Value {
    json!({"friends":[{"friendship_id":"friend", "my_receiver":{
        "installation_id":installation,
        "enc_key_id":null,"machine_label":installation,
        "revision":9,"receiving":false
    }}]})
}

#[test]
fn setup_reads_only_the_named_friend_without_an_account_fallback() {
    let status = friend_status(&list(None), "friend", "here").unwrap();
    assert_eq!(status.receiver_revision, 9);
    assert!(!status.receiving_here());
    assert_eq!(friend_status(&list(None), "foreign", "here"), Err(Error::FriendshipEnded));
    assert_eq!(friend_status(&json!({"friends":[{"friendship_id":"friend"}]}), "friend", "here"), Err(Error::InvalidEnvelope));
}

#[test]
fn another_friends_receiver_does_not_block_setup() {
    let mut list = list(None);
    list["friends"].as_array_mut().unwrap().push(json!({"friendship_id":"other", "my_receiver":{}}));
    assert!(friend_status(&list, "friend", "here").unwrap().receiver_installation_id.is_none());
}

#[test]
fn refusal_names_the_friend_and_machine_and_requires_owner_consent() {
    let friend = friend_status(&list(Some("Other desk")), "friend", "here").unwrap();
    let refusal = receiver_refusal("friend", "Ada", &friend);
    assert!(refusal.contains("Ada is received on Other desk. Move here?"));
    assert!(refusal.contains("--expected-receiver-revision 9 --takeover-confirmed"));
}
