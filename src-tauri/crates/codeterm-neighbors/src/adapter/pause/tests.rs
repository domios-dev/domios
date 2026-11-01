use super::*;

struct Calls {
    reply: Result<Value>,
    seen: std::sync::Mutex<Vec<Value>>,
}

#[async_trait]
impl AuthenticatedCall for Calls {
    async fn invoke(&self, route: &str, data: Value) -> Result<Value> {
        assert_eq!(route, "friendsSetPaused");
        self.seen.lock().unwrap().push(data);
        self.reply.clone()
    }
    async fn id_token(&self) -> Result<Zeroizing<String>> {
        Err(Error::Transport)
    }
}

#[tokio::test]
async fn pause_and_resume_use_the_own_installation_and_require_an_exact_echo() {
    for paused in [true, false] {
        let friend = "a".repeat(64);
        for reply in [
            Ok(json!({"schema_version":1,"friendship_id":friend,"paused":paused})),
            Ok(json!({"schema_version":1,"friendship_id":friend,"paused":!paused})),
            Ok(json!({"schema_version":1,"friendship_id":"b".repeat(64),"paused":paused})),
            Err(Error::SessionRevoked),
        ] {
            let accepted = reply
                .as_ref()
                .is_ok_and(|reply| reply["friendship_id"] == friend && reply["paused"] == paused);
            let calls = Arc::new(Calls {
                reply,
                seen: Default::default(),
            });
            let transport = FirebaseTransport {
                calls: calls.clone(),
                project: "fixture".into(),
                uid: "owner".into(),
                installation_id: "own-machine".into(),
            };
            assert_eq!(
                transport.set_paused(&friend, paused).await.is_ok(),
                accepted
            );
            assert_eq!(
                *calls.seen.lock().unwrap(),
                vec![
                    json!({"installation_id":"own-machine","friendship_id":friend,"paused":paused})
                ]
            );
            assert_eq!(
                transport.set_paused("../foreign", paused).await,
                Err(Error::InvalidRequest)
            );
            assert_eq!(calls.seen.lock().unwrap().len(), 1);
        }
    }
}
