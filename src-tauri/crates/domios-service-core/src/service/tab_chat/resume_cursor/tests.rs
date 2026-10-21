use super::*;

fn epoch() -> ChatEpoch {
    ChatEpoch {
        source_session_id: "supplied-session".into(),
        session_binding_nonce: Some("supplied-binding".into()),
    }
}

fn cursor(offset: u64) -> ResumeCursor {
    ResumeCursor::Bytes {
        epoch: epoch(),
        incarnation: "supplied-incarnation".into(),
        offset,
    }
}

#[test]
fn cursor_source_matrix_never_mislabels_unsupported_as_invalid() {
    let cursors = [
        cursor(100),
        ResumeCursor::Revision {
            epoch: epoch(),
            token: "opaque-provider-token".into(),
        },
        ResumeCursor::Items {
            epoch: epoch(),
            count: 10,
            journal_seq: 12,
        },
    ];
    for source in [
        ResumeSourceKind::Bytes,
        ResumeSourceKind::Revision,
        ResumeSourceKind::Items,
    ] {
        for supplied in &cursors {
            let supported = source == ResumeSourceKind::Bytes && supplied.source_kind() == source;
            assert_eq!(
                validate_source(supplied, source),
                if supported { Ok(()) } else { Err(source) }
            );
        }
    }
}

#[test]
fn valid_cursor_reads_only_the_forward_page_and_reports_empty_at_head() {
    assert_eq!(
        plan_resume(
            &cursor(100),
            &epoch(),
            "supplied-incarnation",
            100,
            true,
            1024
        ),
        ResumePlan::Empty
    );
    assert_eq!(
        plan_resume(
            &cursor(100),
            &epoch(),
            "supplied-incarnation",
            4096,
            true,
            1024
        ),
        ResumePlan::Read {
            from: 100,
            through: 1124
        }
    );
    assert_eq!(
        plan_resume(
            &cursor(4000),
            &epoch(),
            "supplied-incarnation",
            4096,
            true,
            1024
        ),
        ResumePlan::Read {
            from: 4000,
            through: 4096
        }
    );
}

#[test]
fn session_or_binding_change_requires_an_epoch_reset() {
    for changed in [
        ChatEpoch {
            source_session_id: "other-session".into(),
            ..epoch()
        },
        ChatEpoch {
            session_binding_nonce: Some("other-binding".into()),
            ..epoch()
        },
    ] {
        assert_eq!(
            plan_resume(
                &cursor(100),
                &changed,
                "supplied-incarnation",
                4096,
                true,
                1024
            ),
            ResumePlan::Reset(ResumeResetReason::EpochChanged)
        );
    }
}

#[test]
fn truncate_and_regrow_requires_reset_even_when_length_exceeds_old_cursor() {
    assert_eq!(
        plan_resume(
            &cursor(100),
            &epoch(),
            "replacement-incarnation",
            4096,
            true,
            1024
        ),
        ResumePlan::Reset(ResumeResetReason::Compacted)
    );
}

#[test]
fn invalid_offset_boundary_and_zero_budget_never_read() {
    for (offset, boundary, budget) in [(4097, true, 1024), (100, false, 1024), (100, true, 0)] {
        assert_eq!(
            plan_resume(
                &cursor(offset),
                &epoch(),
                "supplied-incarnation",
                4096,
                boundary,
                budget
            ),
            ResumePlan::Reset(ResumeResetReason::InvalidCursor)
        );
    }
}

#[test]
fn cursor_round_trip_preserves_issued_authority_and_rejects_malformed_tokens() {
    let supplied = cursor(100);
    assert_eq!(
        ResumeCursor::decode(&supplied.encode().unwrap()),
        Ok(supplied)
    );
    for invalid in ["not-json".to_owned(), "x".repeat(4097), "{}".to_owned()] {
        assert_eq!(
            ResumeCursor::decode(&invalid),
            Err(ResumeResetReason::InvalidCursor)
        );
    }
}
