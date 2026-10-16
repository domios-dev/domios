use super::*;

fn fixture(
    body: &[u8],
    offset: u64,
) -> (
    tempfile::NamedTempFile,
    JsonlSource,
    ChatEpoch,
    ResumeCursor,
) {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), body).unwrap();
    let source = JsonlSource::default();
    let epoch = ChatEpoch {
        source_session_id: "supplied-session".into(),
        session_binding_nonce: Some("supplied-nonce".into()),
    };
    let incarnation = capture_incarnation(&source, &epoch, file.path(), offset, None).unwrap();
    let token = encode_cursor(epoch.clone(), &incarnation, offset).unwrap();
    let cursor = ResumeCursor::decode(&token).unwrap();
    (file, source, epoch, cursor)
}

#[test]
fn caught_up_resume_never_invokes_the_transcript_parser() {
    let (file, source, epoch, cursor) = fixture(b"{}\n", 3);
    let result = read_page_with(&source, file.path(), &epoch, &cursor, |_, _| {
        panic!("idle resume must not parse history")
    })
    .unwrap();
    assert!(
        matches!(result, ChatResumeResult::Resumed { deltas, caught_up: true } if deltas.is_empty())
    );
}

#[test]
fn growth_after_the_page_read_is_left_for_the_live_tailer_without_resetting() {
    use std::io::Write;
    let record = format!("{}\n", "x".repeat(100));
    let (file, source, epoch, cursor) = fixture(record.repeat(2).as_bytes(), record.len() as u64);
    let result = read_page_with(&source, file.path(), &epoch, &cursor, |from, budget| {
        let chunk =
            codeterm_plugin::jsonl_window::read_records_from(file.path(), from, budget).unwrap();
        let mut writer = std::fs::OpenOptions::new()
            .append(true)
            .open(file.path())
            .unwrap();
        writer.write_all(record.as_bytes()).unwrap();
        Some(crate::agent::chat_types::SessionDelta {
            new_offset: chunk.new_offset,
            messages: vec![],
        })
    })
    .unwrap();
    let ChatResumeResult::Resumed {
        deltas,
        caught_up: true,
    } = result
    else {
        panic!("growth must not reset the lease");
    };
    assert_eq!(deltas[0].offset, record.len() as u64 * 2);
    assert!(deltas[0].offset < std::fs::metadata(file.path()).unwrap().len());
}

#[test]
fn forward_resume_uses_one_budgeted_read_and_reports_the_true_end() {
    let body = "{}\n".repeat(RESUME_PAGE_BYTES as usize);
    let (file, source, epoch, cursor) = fixture(body.as_bytes(), 3);
    let result = read_page_with(&source, file.path(), &epoch, &cursor, |from, budget| {
        assert_eq!(from, 3);
        assert_eq!(budget, RESUME_PAGE_BYTES);
        let chunk =
            codeterm_plugin::jsonl_window::read_records_from(file.path(), from, budget).unwrap();
        Some(crate::agent::chat_types::SessionDelta {
            new_offset: chunk.new_offset,
            messages: vec![],
        })
    })
    .unwrap();
    let ChatResumeResult::Resumed {
        deltas,
        caught_up: false,
    } = result
    else {
        panic!("expected bounded continuation");
    };
    assert_eq!(deltas.len(), 1);
    assert_eq!(deltas[0].start_offset, 3);
    assert!(deltas[0].offset <= 3 + RESUME_PAGE_BYTES);
    assert_eq!(deltas[0].offset % 3, 0);
    let ResumeCursor::Bytes { offset, .. } = ResumeCursor::decode(&deltas[0].cursor).unwrap()
    else {
        panic!("expected byte cursor");
    };
    assert_eq!(offset, deltas[0].offset);
}

#[test]
fn changed_incarnation_and_epoch_reset_without_parsing() {
    let (file, source, epoch, cursor) = fixture(b"old-record\n", 11);
    std::fs::write(file.path(), b"replacement-record\nmore\n").unwrap();
    let result = read_page_with(&source, file.path(), &epoch, &cursor, |_, _| {
        panic!("compacted source must not parse")
    })
    .unwrap();
    assert!(matches!(
        result,
        ChatResumeResult::ResetRequired {
            reason: ResumeResetReason::Compacted
        }
    ));
    let (file, source, mut epoch, cursor) = fixture(b"{}\n", 3);
    epoch.session_binding_nonce = Some("replacement-nonce".into());
    let result = read_page_with(&source, file.path(), &epoch, &cursor, |_, _| {
        panic!("foreign epoch must not parse")
    })
    .unwrap();
    assert!(matches!(
        result,
        ChatResumeResult::ResetRequired {
            reason: ResumeResetReason::EpochChanged
        }
    ));
}

#[test]
fn unsupported_budgeted_parser_does_not_fall_back_to_an_unbounded_read() {
    let (file, source, epoch, cursor) = fixture(b"{}\n{}\n", 3);
    let result = read_page_with(&source, file.path(), &epoch, &cursor, |_, _| None).unwrap();
    assert!(matches!(
        result,
        ChatResumeResult::Unsupported {
            source_kind: ResumeSourceKind::Bytes
        }
    ));
}
