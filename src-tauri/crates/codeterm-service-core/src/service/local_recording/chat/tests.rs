use super::*;
use std::cell::RefCell;

fn identity(session: &str, nonce: &str) -> ChatIdentity {
    ChatIdentity {
        tab_id: "A".into(),
        plugin_id: "fixture".into(),
        session_id: session.into(),
        binding_nonce: nonce.into(),
    }
}

fn capture() -> CapturedChat {
    CapturedChat {
        identity: identity("S", "N"),
        active: Arc::new(AtomicBool::new(true)),
        expires_at: std::time::Instant::now() + std::time::Duration::from_secs(600),
    }
}

#[test]
fn lark_recording_contract_chat_worker_rebind_baseline_and_frozen_handoff() {
    let capture = capture();
    let current = RefCell::new(Some(capture.identity.clone()));
    let writes = RefCell::new(Vec::new());
    capture.validate(current.borrow().clone()).unwrap();
    *current.borrow_mut() = Some(identity("replacement", "rotated"));

    // Models the existing manual worker's tab lookup after tracked admission.
    let manual_identity = current.borrow().clone().unwrap();
    writes.borrow_mut().push(manual_identity.session_id);
    assert_eq!(*writes.borrow(), ["replacement"]);
    writes.borrow_mut().clear();

    let result = capture.handoff(current.borrow().clone(), |_, session| {
        writes.borrow_mut().push(session.to_string());
        Ok(())
    });
    assert!(result.is_err());
    assert!(writes.borrow().is_empty());
}

#[test]
fn lark_recording_contract_chat_unchanged_hands_off_exact_session_once() {
    let capture = capture();
    let writes = RefCell::new(Vec::new());
    capture
        .handoff(Some(capture.identity.clone()), |plugin, session| {
            writes
                .borrow_mut()
                .push((plugin.to_string(), session.to_string()));
            Ok(())
        })
        .unwrap();
    assert_eq!(*writes.borrow(), [("fixture".into(), "S".into())]);
}

#[test]
fn lark_recording_contract_chat_cancel_revoke_close_and_nonce_change_refuse() {
    for current in [
        None,
        Some(identity("S", "rotated")),
        Some(identity("other", "N")),
    ] {
        let capture = capture();
        assert!(capture
            .handoff(current, |_, _| panic!("stale handoff"))
            .is_err());
    }
    let capture = capture();
    capture.active.store(false, Ordering::Release);
    assert!(capture
        .handoff(Some(capture.identity.clone()), |_, _| panic!(
            "revoked handoff"
        ))
        .is_err());
}

#[test]
fn lark_recording_contract_chat_backend_error_is_uncertain_without_retry() {
    let capture = capture();
    let writes = RefCell::new(0);
    let error = capture
        .handoff(Some(capture.identity.clone()), |_, _| {
            *writes.borrow_mut() += 1;
            Err("possible handoff".into())
        })
        .unwrap_err();
    assert!(error.bytes_written());
    assert_eq!(*writes.borrow(), 1);
}

#[test]
fn lark_recording_contract_chat_tracked_admission_stale_final_handoff_and_uncertainty() {
    use crate::service::chat_send_transaction::{
        run_chat_send, ChatSendLedger, ChatSendState, SendLiveBinding,
    };
    for stale in [true, false] {
        let capture = capture();
        let ledger = ChatSendLedger::default();
        capture.validate(Some(capture.identity.clone())).unwrap();
        let pending = ledger
            .try_start_scoped(
                "A",
                "voice-token",
                None,
                Some(SendLiveBinding {
                    session_id: Some("S".into()),
                    binding_nonce: Some("N".into()),
                }),
            )
            .unwrap();
        let pending = ledger
            .attach_client_token(&pending.send_id, "opaque-capture-client".into())
            .unwrap();
        let current = if stale {
            Some(identity("replacement", "rotated"))
        } else {
            Some(capture.identity.clone())
        };
        let writes = RefCell::new(0);
        let status = run_chat_send(&ledger, pending, "voice-token", |_| {
            capture.handoff(current, |_, _| {
                *writes.borrow_mut() += 1;
                Err("backend may have accepted".into())
            })
        });
        assert_eq!(
            status.correlation_id.as_deref(),
            Some("opaque-capture-client")
        );
        assert_eq!(*writes.borrow(), usize::from(!stale));
        assert_eq!(
            status.state,
            if stale {
                ChatSendState::Failed
            } else {
                ChatSendState::Unknown
            }
        );
        ledger.reconcile("A");
        assert_eq!(*writes.borrow(), usize::from(!stale));
    }
}

#[test]
fn lark_recording_contract_existing_identity_only_guard_cannot_revoke_capture() {
    let capture = capture();
    let admitted_identity = capture.identity.clone();
    capture.active.store(false, Ordering::Release);
    let current_identity = admitted_identity.clone();
    let mut baseline_handoffs = 0;
    if current_identity == admitted_identity {
        baseline_handoffs += 1;
    }
    assert_eq!(baseline_handoffs, 1);
    let mut captured_handoffs = 0;
    assert!(capture
        .handoff(Some(current_identity), |_, _| {
            captured_handoffs += 1;
            Ok(())
        })
        .is_err());
    assert_eq!(captured_handoffs, 0);
}
