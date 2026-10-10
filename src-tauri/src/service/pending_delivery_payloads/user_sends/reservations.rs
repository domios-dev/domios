use super::*;

impl UserSendStore {
    #[cfg(test)]
    pub(crate) fn reserve(
        &self,
        authority: UserSendAuthority,
        client_token: Option<String>,
        allocate_identity: impl FnOnce() -> (String, u64),
    ) -> Result<UserReservation, String> {
        self.reserve_with_replay_only(authority, client_token, false, allocate_identity)
    }

    pub(crate) fn reserve_with_replay_only(
        &self,
        authority: UserSendAuthority,
        client_token: Option<String>,
        replay_only: bool,
        allocate_identity: impl FnOnce() -> (String, u64),
    ) -> Result<UserReservation, String> {
        if client_token
            .as_ref()
            .is_some_and(|token| token.len() as u64 > MAX_INDEX_BYTES)
        {
            return Err("user correlation token exceeds metadata bound".into());
        }
        let tab = self.tab(&authority.tab_id);
        let mut admissions = tab.admissions.lock();
        if let Some(entry) = client_token
            .as_ref()
            .and_then(|token| admissions.tokens.existing(token))
        {
            drop(admissions);
            return Ok(UserReservation {
                send_id: entry.send_id.clone(),
                ledger_sequence: entry.sequence,
                authority,
                client_token,
                ordinal: 0,
                tab,
                reused: true,
                token_entry: Some(entry),
            });
        }
        if replay_only {
            return Err("admission_unknown: replay token has no retained admission".into());
        }
        if tab.revoked.load(Ordering::Acquire) || tab.failed.load(Ordering::Acquire) {
            return Err("user send owner is held or revoked".into());
        }
        if admissions.pending.len() + tab.unsettled.load(Ordering::Acquire) >= MAX_UNSETTLED {
            return Err("user send capacity: 32 unsettled admissions".into());
        }
        let ordinal = admissions.next;
        admissions.next = ordinal
            .checked_add(1)
            .ok_or("user admission sequence exhausted")?;
        let (send_id, ledger_sequence) = allocate_identity();
        let token_entry = client_token
            .as_ref()
            .map(|token| {
                admissions
                    .tokens
                    .insert(token.clone(), send_id.clone(), ledger_sequence)
            })
            .transpose()?;
        admissions.pending.insert(ordinal);
        drop(admissions);
        Ok(UserReservation {
            send_id,
            ledger_sequence,
            authority,
            client_token,
            ordinal,
            tab,
            reused: false,
            token_entry,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::chat_send_transaction::{ChatSendLedger, ChatSendState};

    fn authority(tab: &str) -> UserSendAuthority {
        UserSendAuthority {
            tab_id: tab.into(),
            pty_id: Some("pty".into()),
            binding_nonce: "nonce".into(),
            pty_session_generation: Some(1),
            provider_id: "opaque".into(),
            session_id: None,
            observed_session_id: None,
            prelaunch: false,
        }
    }

    #[test]
    fn terminal_token_returns_delivered_or_failed_without_new_delivery() {
        for delivered in [true, false] {
            let temp = tempfile::tempdir().unwrap();
            let store = UserSendStore::open(temp.path()).unwrap();
            let ledger = ChatSendLedger::default();
            let first = store
                .reserve(authority("tab"), Some("token".into()), || {
                    let status = ledger.try_start("tab", "opaque payload", None).unwrap();
                    (status.send_id, status.sequence)
                })
                .unwrap();
            let send_id = first.send_id.clone();
            drop(first);
            let terminal = if delivered {
                ledger.confirm_send(&send_id).unwrap()
            } else {
                ledger
                    .finish(&send_id, Err("proven refusal".into()))
                    .unwrap()
            };
            assert_eq!(
                terminal.state,
                if delivered {
                    ChatSendState::Sent
                } else {
                    ChatSendState::Failed
                }
            );
            let duplicate = store
                .reserve(authority("tab"), Some("token".into()), || {
                    panic!("duplicate allocated identity")
                })
                .unwrap();
            assert!(duplicate.wait_existing().unwrap());
            assert_eq!(duplicate.send_id, send_id);
            assert_eq!(ledger.status(&duplicate.send_id).unwrap(), terminal);
            assert!(store.records("tab").unwrap().is_empty());
        }
    }

    #[test]
    fn duplicate_admission_waits_for_owner_and_does_not_release_its_ordering_slot() {
        let temp = tempfile::tempdir().unwrap();
        let store = UserSendStore::open(temp.path()).unwrap();
        let first = store
            .reserve(authority("tab"), Some("token".into()), || {
                (uuid::Uuid::new_v4().to_string(), 1)
            })
            .unwrap();
        let duplicate = store
            .reserve(authority("tab"), Some("token".into()), || {
                panic!("duplicate allocated identity")
            })
            .unwrap();
        assert_eq!(duplicate.send_id, first.send_id);
        drop(duplicate);
        assert_eq!(first.tab.admissions.lock().pending.len(), 1);
        let duplicate = store
            .reserve(authority("tab"), Some("token".into()), || {
                panic!("duplicate allocated identity")
            })
            .unwrap();
        let (complete, completed) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            complete.send(duplicate.wait_existing()).unwrap();
        });
        drop(first);
        assert!(completed
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap()
            .unwrap());
        worker.join().unwrap();
        let fresh = store
            .reserve(authority("other-tab"), Some("token".into()), || {
                (uuid::Uuid::new_v4().to_string(), 2)
            })
            .unwrap();
        assert!(!fresh.wait_existing().unwrap());
    }

    #[test]
    fn evicted_or_absent_replay_token_refuses_without_allocating_but_fresh_sends_continue() {
        let temp = tempfile::tempdir().unwrap();
        let store = UserSendStore::open(temp.path()).unwrap();
        for index in 0..=256 {
            drop(
                store
                    .reserve(authority("tab"), Some(format!("token-{index}")), || {
                        (uuid::Uuid::new_v4().to_string(), index)
                    })
                    .unwrap(),
            );
        }
        for token in [Some("token-0".into()), Some("unknown".into()), None] {
            let error = store
                .reserve_with_replay_only(authority("tab"), token, true, || {
                    panic!("unknown replay allocated identity")
                })
                .err()
                .unwrap();
            assert!(error.starts_with("admission_unknown:"));
        }
        let known = store
            .reserve_with_replay_only(authority("tab"), Some("token-256".into()), true, || {
                panic!("known replay allocated identity")
            })
            .unwrap();
        assert!(known.wait_existing().unwrap());
        let fresh = store
            .reserve(authority("tab"), Some("fresh".into()), || {
                (uuid::Uuid::new_v4().to_string(), 258)
            })
            .unwrap();
        assert!(!fresh.wait_existing().unwrap());
    }
}
