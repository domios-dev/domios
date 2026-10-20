use super::*;

#[test]
fn stale_send_anchor_stays_unconfirmed_without_parsing_history() {
    let fx = Fixture::new();
    let path = fx.session_path("own");
    let from = 48_789_141;
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(from).unwrap();
    let anchor = fx.anchor_now("own");
    let send = fx
        .ledger
        .try_start("tab", "opaque", Some(anchor.clone()))
        .unwrap();
    fx.ledger
        .finish(&send.send_id, Err("submit_unconfirmed: no evidence".into()))
        .unwrap();
    file.set_len(from + codeterm_plugin::jsonl_window::DELTA_CHUNK_BYTES + 1)
        .unwrap();
    fx.append_user_turn("own", "opaque");
    assert!(fx.scan("tab").is_empty());
    assert!(!contains_exact_user_turn_since(
        &fx.registry,
        &anchor,
        "opaque",
        &mut ExactTurnCursor::for_anchor(Some(&anchor))
    ));
    assert!(fx.parse_offsets.lock().unwrap().is_empty());
    assert_eq!(
        fx.ledger.status(&send.send_id).unwrap().state,
        ChatSendState::Unknown
    );
}

#[test]
fn claimed_old_anchor_is_not_reread_for_a_new_send() {
    let fx = Fixture::new();
    let old = fx
        .ledger
        .try_start("tab", "opaque", Some(fx.anchor_now("own")))
        .unwrap();
    fx.append_user_turn("own", "opaque");
    assert_eq!(fx.scan("tab").len(), 1);
    assert_eq!(
        fx.ledger.status(&old.send_id).unwrap().state,
        ChatSendState::Sent
    );
    std::fs::OpenOptions::new()
        .write(true)
        .open(fx.session_path("own"))
        .unwrap()
        .set_len(codeterm_plugin::jsonl_window::DELTA_CHUNK_BYTES + 1)
        .unwrap();
    let anchor = fx.anchor_now("own");
    let EvidenceWindow::Jsonl { transcript_offset } = anchor.window else {
        unreachable!()
    };
    let send = fx.ledger.try_start("tab", "opaque", Some(anchor)).unwrap();
    fx.ledger
        .finish(&send.send_id, Err("submit_unconfirmed: no evidence".into()))
        .unwrap();
    fx.append_user_turn("own", "opaque");
    fx.parse_offsets.lock().unwrap().clear();
    assert!(fx.scan("tab").is_empty());
    assert!(fx
        .parse_offsets
        .lock()
        .unwrap()
        .iter()
        .all(|offset| *offset == transcript_offset));
    assert_eq!(
        fx.ledger.status(&send.send_id).unwrap().state,
        ChatSendState::Unknown
    );
}
