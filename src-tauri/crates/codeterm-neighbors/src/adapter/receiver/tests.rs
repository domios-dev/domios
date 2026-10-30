use super::*;

#[test]
fn friend_receiver_authority_requires_this_machine_and_enabled_receiving() {
    let mut status = ReceiverStatus { is_me: true, receiving_enabled: true, ..Default::default() };
    assert!(status.receiving_here());
    status.is_me = false;
    assert!(!status.receiving_here());
    status.is_me = true;
    status.receiving_enabled = false;
    assert!(!status.receiving_here());
}

#[tokio::test]
async fn status_is_a_per_friend_list_including_unassigned_friends() {
    use crate::adapter::{AuthenticatedCall, FirebaseRegistry};
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use std::sync::Arc;
    use zeroize::Zeroizing;
    struct Calls;
    #[async_trait]
    impl AuthenticatedCall for Calls {
        async fn invoke(&self, route: &str, data: Value) -> crate::Result<Value> {
            assert_eq!(route, "friendsReceiverStatus");
            assert_eq!(data, json!({"installation_id":"machine"}));
            Ok(json!({"schema_version":1,"receivers":[
                {"friendship_id":"a".repeat(64),"machine_label":null,"revision":0,"receiving":false,"is_me":false},
                {"friendship_id":"b".repeat(64),"machine_label":"Other desk","revision":9,"receiving":true,"is_me":false}
            ]}))
        }
        async fn id_token(&self) -> crate::Result<Zeroizing<String>> { panic!("no authentication") }
    }
    let registry = FirebaseRegistry { calls: Arc::new(Calls), uid: "owner".into(), installation_id: "machine".into(), machine_label: "Desk".into() };
    let rows = registry.receiver_status().await.unwrap();
    assert_eq!(rows.len(),2);
    assert_eq!(rows[0].revision,0);
    assert!(!rows[0].receiving);
    assert_eq!(rows[1].machine_label.as_deref(),Some("Other desk"));
    assert!(!rows[1].is_me);
}
