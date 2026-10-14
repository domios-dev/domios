use super::*;

#[test]
fn structured_blank_line_merged_turn_confirms_sends_across_append_cursors() {
    let fixture = StructuredFixture::new();
    fixture.append_user_row("session-1", "prior turn");
    let first = fixture
        .ledger
        .try_start(
            "tab-1",
            "alpha fresh body",
            Some(fixture.anchor_now("session-1")),
        )
        .unwrap();
    fail_send(&fixture.ledger, &first);

    fixture.append_user_row("session-1", "unrelated turn between sends");
    let second = fixture
        .ledger
        .try_start(
            "tab-1",
            "beta fresh body",
            Some(fixture.anchor_now("session-1")),
        )
        .unwrap();
    fail_send(&fixture.ledger, &second);

    fixture.append_user_row("session-1", "another unrelated turn");
    let third = fixture
        .ledger
        .try_start(
            "tab-1",
            "gamma fresh body",
            Some(fixture.anchor_now("session-1")),
        )
        .unwrap();
    fail_send(&fixture.ledger, &third);
    fixture.append_user_row(
        "session-1",
        "alpha fresh body\n\nbeta fresh body\n\ngamma fresh body",
    );

    let confirmed = fixture.scan("tab-1");
    assert_eq!(
        confirmed
            .iter()
            .map(|status| status.send_id.as_str())
            .collect::<Vec<_>>(),
        [
            first.send_id.as_str(),
            second.send_id.as_str(),
            third.send_id.as_str()
        ],
    );
    assert!(confirmed
        .iter()
        .all(|status| status.state == ChatSendState::Sent));
}
