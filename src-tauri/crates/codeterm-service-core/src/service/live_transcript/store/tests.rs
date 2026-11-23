use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use tempfile::{tempdir, TempDir};

use super::super::*;
use super::{encoded_record_path, raw_path, read_private, MAX_RECORD_BYTES};

const OWNER: &str = "owner-1";

fn fixture(budget: u64) -> (TempDir, TranscriptStore) {
    let root = tempdir().unwrap();
    let store = TranscriptStore::open(root.path(), OWNER, budget).unwrap();
    (root, store)
}

fn speech(text: &str) -> TranscriptInput {
    TranscriptInput {
        at_ms: 123,
        content: TranscriptContent::Speech {
            text: text.to_owned(),
        },
    }
}

#[test]
fn recent_context_keeps_the_latest_sixteen_speech_rows_in_order() {
    let (_temp, store) = fixture(DEFAULT_RAW_BUDGET_BYTES);
    for _ in 0..20 {
        append(&store, "Speech chunk");
        store
            .append(
                OWNER,
                TranscriptInput {
                    at_ms: 123,
                    content: TranscriptContent::Resumed,
                },
            )
            .unwrap();
    }
    append(&store, "Future speech");
    let sequences = store
        .recent_context(40)
        .unwrap()
        .into_iter()
        .map(|row| row.seq)
        .collect::<Vec<_>>();
    assert_eq!(sequences, (9..40).step_by(2).collect::<Vec<_>>());
}

#[test]
fn forward_receipts_persist_and_return_the_latest_fifty_in_time_order() {
    let (temp, store) = fixture(DEFAULT_RAW_BUDGET_BYTES);
    assert!(store.forwards().unwrap().is_empty());
    append(&store, "Speech chunk");
    let forward = |at_ms| TranscriptForward {
        forward_id: format!("forward-{at_ms}"),
        epoch_id: Some("epoch-1".into()),
        range: TranscriptRange {
            from_seq: 1,
            to_seq: 1,
        },
        at_ms,
        target_tab_id: "tab-1".into(),
        target_title: "Main".into(),
        sent_text: "Please send this".into(),
        sent_excerpt: "Please send this".into(),
        rationale: None,
    };
    for at_ms in (1..=55).rev() {
        store.record_forward(forward(at_ms)).unwrap();
    }
    let expected = (6..=55).map(forward).collect::<Vec<_>>();
    assert_eq!(store.forwards().unwrap(), expected);
    drop(store);

    let reopened = TranscriptStore::open(temp.path(), OWNER, DEFAULT_RAW_BUDGET_BYTES).unwrap();
    assert_eq!(reopened.forwards().unwrap(), expected);
}

fn append(store: &TranscriptStore, text: &str) -> AppendReceipt {
    store.append(OWNER, speech(text)).unwrap()
}

fn epoch(id: &str, runtime: &str) -> ConciergeEpoch {
    ConciergeEpoch {
        epoch_id: id.to_owned(),
        runtime_tab_id: runtime.to_owned(),
        session_id: format!("session-{id}"),
        binding_nonce: format!("nonce-{id}"),
        provider: "chat-provider".to_owned(),
        model: None,
        started_at_ms: 100,
    }
}

fn attach(store: &TranscriptStore, epoch: ConciergeEpoch) -> TranscriptBinding {
    let current = store.binding();
    let target = BindingTarget::Attached {
        epoch_id: epoch.epoch_id.clone(),
    };
    store.stage_epoch(current.revision, epoch).unwrap();
    store
        .set_binding(current.revision, target, CursorStart::Beginning)
        .unwrap()
}

fn switch(store: &TranscriptStore, target: BindingTarget, start: CursorStart) -> TranscriptBinding {
    store
        .set_binding(store.binding().revision, target, start)
        .unwrap()
}

fn external(id: &str) -> BindingTarget {
    BindingTarget::External { tab_id: id.into() }
}

fn closed(tab_id: &str, group_id: &str, closed_at_ms: u64) -> ClosedTranscriptOwner {
    ClosedTranscriptOwner {
        tab_id: tab_id.into(),
        group_id: group_id.into(),
        closed_at_ms,
    }
}

fn root(path: &Path, tab: &str) -> std::path::PathBuf {
    path.canonicalize().unwrap().join("transcripts").join(tab)
}

fn summarize(store: &TranscriptStore, range: TranscriptRange, text: &str) {
    store
        .write_summary(SegmentSummary {
            range,
            text: text.into(),
        })
        .unwrap();
}

#[test]
fn torn_tail_and_interrupted_metadata_preserve_the_committed_prefix() {
    let (temp, store) = fixture(DEFAULT_RAW_BUDGET_BYTES);
    let active = attach(&store, epoch("epoch-1", "router-a"));
    assert_eq!(append(&store, "committed one").row.seq, 1);
    assert_eq!(append(&store, "committed two").row.seq, 2);
    store.ack(&active, 1).unwrap();
    let dir = root(temp.path(), OWNER);
    let raw = raw_path(&dir, 1);
    let prefix = fs::read(&raw).unwrap();
    drop(store);

    let mut file = OpenOptions::new().append(true).open(&raw).unwrap();
    file.write_all(br#"{"seq":3,"atMs":123,"content":{"kind":"speech","text":"torn"#)
        .unwrap();
    drop(file);
    fs::write(dir.join(".manifest.json.interrupted.tmp"), b"{").unwrap();
    fs::write(
        dir.join(".binding.json.interrupted.tmp"),
        b"{\"other\":true}",
    )
    .unwrap();
    fs::write(dir.join("concierge_epochs").join(".interrupted.tmp"), b"{").unwrap();

    let store = TranscriptStore::open(temp.path(), OWNER, DEFAULT_RAW_BUDGET_BYTES).unwrap();
    assert_eq!(fs::read(&raw).unwrap(), prefix);
    assert_eq!(store.binding(), active);
    assert_eq!(store.cursor(&active).unwrap().acked_seq, 1);
    assert_eq!(store.history(1, 10).unwrap().rows.len(), 2);
    assert_eq!(append(&store, "committed after restart").row.seq, 3);
    assert_eq!(
        store.epoch("epoch-1").unwrap().unwrap().epoch,
        epoch("epoch-1", "router-a")
    );
    assert!(!fs::read_to_string(raw).unwrap().contains("torn"));
}

#[test]
fn terminated_corrupt_history_is_refused_without_truncation() {
    let (temp, store) = fixture(DEFAULT_RAW_BUDGET_BYTES);
    append(&store, "persist");
    let raw = raw_path(&root(temp.path(), OWNER), 1);
    drop(store);
    let mut file = OpenOptions::new().append(true).open(&raw).unwrap();
    file.write_all(b"{invalid}\n").unwrap();
    drop(file);
    let before = fs::read(&raw).unwrap();
    assert!(TranscriptStore::open(temp.path(), OWNER, DEFAULT_RAW_BUDGET_BYTES).is_err());
    assert_eq!(fs::read(raw).unwrap(), before);
}

#[test]
fn row_rotation_seals_at_one_thousand_and_recovers_next_sequence() {
    let (temp, store) = fixture(DEFAULT_RAW_BUDGET_BYTES);
    let raw = raw_path(&root(temp.path(), OWNER), 1);
    drop(store);
    let mut file = codeterm_util::private_file::create_new_owner_only(&raw).unwrap();
    for seq in 1..RAW_SEGMENT_ROWS as u64 {
        let input = speech("bounded row");
        serde_json::to_writer(
            &mut file,
            &TranscriptRow {
                seq,
                at_ms: input.at_ms,
                content: input.content,
            },
        )
        .unwrap();
        file.write_all(b"\n").unwrap();
    }
    file.sync_all().unwrap();
    drop(file);

    let store = TranscriptStore::open(temp.path(), OWNER, DEFAULT_RAW_BUDGET_BYTES).unwrap();
    let receipt = append(&store, "last row in segment");
    assert_eq!(receipt.row.seq, 1000);
    assert_eq!(
        receipt.sealed,
        vec![TranscriptRange {
            from_seq: 1,
            to_seq: 1000
        }]
    );
    assert_eq!(store.read_sealed(receipt.sealed[0]).unwrap().len(), 1000);
    assert_eq!(append(&store, "new segment").row.seq, 1001);
    drop(store);

    let store = TranscriptStore::open(temp.path(), OWNER, DEFAULT_RAW_BUDGET_BYTES).unwrap();
    assert_eq!(store.committed_seq(), 1001);
    assert_eq!(store.pending_summaries(10).unwrap(), receipt.sealed);
    assert_eq!(append(&store, "continues").row.seq, 1002);
}

#[test]
fn byte_rotation_stays_below_limit_and_rejects_encoded_oversize() {
    let (temp, store) = fixture(DEFAULT_RAW_BUDGET_BYTES);
    let text = "a".repeat(MAX_ROW_BYTES - 256);
    let mut ranges = Vec::new();
    for _ in 0..40 {
        ranges.extend(append(&store, &text).sealed);
    }
    assert_eq!(ranges.len(), 1);
    let range = ranges[0];
    assert!(range.to_seq < RAW_SEGMENT_ROWS as u64);
    let bytes = fs::metadata(raw_path(&root(temp.path(), OWNER), 1))
        .unwrap()
        .len();
    assert!(bytes <= RAW_SEGMENT_BYTES && bytes + text.len() as u64 > RAW_SEGMENT_BYTES);
    assert_eq!(store.read_sealed(range).unwrap().len() as u64, range.to_seq);
    assert_eq!(
        store
            .append(OWNER, speech(&"\0".repeat(MAX_ROW_BYTES / 3)))
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(store.committed_seq(), 40);
    assert_eq!(append(&store, "still writable").row.seq, 41);
}

#[test]
fn exact_bindings_and_independent_logical_cursors_survive_runtime_recreation() {
    let (temp, store) = fixture(DEFAULT_RAW_BUDGET_BYTES);
    for _ in 0..4 {
        append(&store, "speech");
    }
    let first_epoch = epoch("epoch-1", "router-a");
    let original = attach(&store, first_epoch.clone());
    assert_eq!(
        store.ack(&original, 2).unwrap().consumer_key,
        "concierge:owner-1"
    );
    assert_eq!(store.mark_seen(&original, 3).unwrap().seen_seq, 3);
    assert_eq!(store.ack(&original, 1).unwrap().acked_seq, 2);
    assert_eq!(store.mark_seen(&original, 2).unwrap().seen_seq, 3);
    assert!(store.ack(&original, 5).is_err());

    let old_staged = epoch("staged-but-never-active", "router-unused");
    store
        .stage_epoch(original.revision, old_staged.clone())
        .unwrap();
    let override_binding = switch(&store, external("outside-1"), CursorStart::After(1));
    assert_eq!(
        store.cursor(&override_binding).unwrap().consumer_key,
        "tab:outside-1"
    );
    assert_eq!(store.cursor(&override_binding).unwrap().acked_seq, 1);
    assert_eq!(store.ack(&override_binding, 4).unwrap().acked_seq, 4);
    assert_eq!(
        store.ack(&original, 3).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert!(store
        .set_binding(
            override_binding.revision,
            BindingTarget::Attached {
                epoch_id: old_staged.epoch_id
            },
            CursorStart::Beginning
        )
        .is_err());
    assert!(store
        .set_binding(original.revision, external("foreign"), CursorStart::Latest)
        .is_err());
    store
        .archive_epoch(
            &first_epoch,
            EpochArchive {
                history_ref: "archive/1".into(),
                ended_at_ms: 200,
            },
        )
        .unwrap();
    for logical_key in ["concierge:owner-1", "tab:outside-1"] {
        let path = encoded_record_path(&root(temp.path(), OWNER).join("cursors"), logical_key);
        let filename = path.file_name().unwrap().to_str().unwrap();
        assert!(filename
            .strip_suffix(".json")
            .unwrap()
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
        assert!(codeterm_util::private_file::owner_only(&path).unwrap());
        let record = read_private::<TranscriptCursor>(&path, MAX_RECORD_BYTES)
            .unwrap()
            .unwrap();
        assert_eq!(record.consumer_key, logical_key);
    }
    drop(store);

    let store = TranscriptStore::open(temp.path(), OWNER, DEFAULT_RAW_BUDGET_BYTES).unwrap();
    assert_eq!(store.cursor(&override_binding).unwrap().acked_seq, 4);
    let new_epoch = attach(&store, epoch("epoch-2", "router-b"));
    let cursor = store.cursor(&new_epoch).unwrap();
    assert_eq!((cursor.acked_seq, cursor.seen_seq), (2, 3));
    assert_eq!(store.attached_cursor().unwrap(), cursor);
    assert!(store.cursor(&override_binding).is_err());
    let saved = store.epoch("epoch-1").unwrap().unwrap();
    assert_eq!(saved.epoch, first_epoch);
    assert_eq!(saved.archive.unwrap().history_ref, "archive/1");
    let external_again = switch(&store, external("outside-1"), CursorStart::Beginning);
    assert_eq!(store.cursor(&external_again).unwrap().acked_seq, 4);
}

#[test]
fn eviction_requires_active_ack_and_durable_summary_and_keeps_truthful_gaps() {
    let (temp, store) = fixture(0);
    let initial = attach(&store, epoch("one", "router-1"));
    append(&store, "raw needle");
    let summarized = store.seal_active(OWNER).unwrap().unwrap();
    summarize(&store, summarized, "exact summary needle");
    append(&store, "unread needle");
    let unread = store.seal_active(OWNER).unwrap().unwrap();
    summarize(&store, unread, "second summary");
    let hot_result = store.search("needle", 1, 2).unwrap();
    assert_eq!(hot_result.hits.len(), 3);
    assert!(hot_result.gaps.is_empty());

    assert!(store.evict_to_budget().unwrap().is_empty());
    let override_binding = switch(&store, external("outside-1"), CursorStart::After(1));
    append(&store, "unacknowledged newest raw");
    assert_eq!(store.evict_to_budget().unwrap(), vec![summarized]);
    assert!(store.raw_bytes() > 0);
    let cold_result = store.search("needle", 1, 10).unwrap();
    assert_eq!(cold_result.gaps, vec![summarized]);
    assert!(
        matches!(&cold_result.hits[..], [TranscriptSearchHit::Summary { summary }, TranscriptSearchHit::Raw { row }] if summary.range == summarized && row.seq == 2)
    );
    assert!(store.read_sealed(summarized).is_err());
    assert_eq!(store.read_sealed(unread).unwrap().len(), 1);
    assert!(store.ack(&initial, 3).is_err());

    store.ack(&override_binding, 3).unwrap();
    let no_summary = store.seal_active(OWNER).unwrap().unwrap();
    switch(&store, BindingTarget::Detached, CursorStart::Beginning);
    assert!(store.evict_to_budget().unwrap().is_empty());
    let reattached = switch(&store, external("outside-1"), CursorStart::Beginning);
    assert_eq!(store.cursor(&reattached).unwrap().acked_seq, 3);
    assert_eq!(store.evict_to_budget().unwrap(), vec![unread]);
    assert_eq!(store.pending_summaries(10).unwrap(), vec![no_summary]);
    drop(store);

    let store = TranscriptStore::open(temp.path(), OWNER, 0).unwrap();
    assert_eq!(store.committed_seq(), 3);
    assert_eq!(append(&store, "after cold recovery").row.seq, 4);
}

#[test]
fn interrupted_eviction_recovers_existing_raw_without_replaying_unlink() {
    let (temp, store) = fixture(0);
    let binding = switch(&store, external("outside-1"), CursorStart::Beginning);
    append(&store, "raw retained");
    let range = store.seal_active(OWNER).unwrap().unwrap();
    summarize(&store, range, "summary durable");
    store.ack(&binding, 1).unwrap();
    {
        let mut state = store.inner.state.lock();
        let mut manifest = state.manifest.clone();
        manifest.cold_ranges.insert(range.from_seq, range);
        store.save_manifest(&mut state, manifest).unwrap();
    }
    drop(store);

    let store = TranscriptStore::open(temp.path(), OWNER, 0).unwrap();
    assert_eq!(store.history(1, 1).unwrap().rows.len(), 1);
    let new_binding = switch(&store, external("outside-2"), CursorStart::Beginning);
    assert!(store.evict_to_budget().unwrap().is_empty());
    store.ack(&new_binding, 1).unwrap();
    assert_eq!(store.evict_to_budget().unwrap(), vec![range]);
    drop(store);
    let store = TranscriptStore::open(temp.path(), OWNER, 0).unwrap();
    assert_eq!(
        append(&store, "next sequence after last segment evicted")
            .row
            .seq,
        2
    );
}

#[test]
fn format_v1_pages_unicode_at_the_smallest_bound_and_keeps_live_ephemeral() {
    let (_temp, store) = fixture(DEFAULT_RAW_BUDGET_BYTES);
    let active = attach(&store, epoch("one", "router-1"));
    for text in [
        "context old",
        "context two",
        "context three",
        "context four",
    ] {
        append(&store, text);
    }
    store.ack(&active, 4).unwrap();
    append(&store, "æ🦉漢");
    append(&store, "unread");
    store.mark_seen(&active, 5).unwrap();
    assert_eq!(
        store
            .recent_context(4)
            .unwrap()
            .iter()
            .map(|row| row.seq)
            .collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );

    let full = store
        .read_page(&active, None, 400, Some("transient 🦉"))
        .unwrap();
    assert!(full.text.contains("[CONTEXT]\n1 | context old"));
    assert!(full.text.contains("[UNREAD]\n6 | unread"));
    assert!(full.text.contains("[SEEN]\n5 | æ🦉漢"));
    assert!(full.text.contains("[LIVE]\ntransient 🦉"));
    assert_eq!(full.through_seq, 6);
    assert_eq!(full.next, None);
    assert!(store.search("transient", 1, 10).unwrap().hits.is_empty());

    let mut next = Some(TranscriptPosition {
        seq: 5,
        char_offset: 0,
    });
    let mut previous = (4, 0);
    let mut glyphs = String::new();
    for _ in 0..32 {
        let position = next.unwrap();
        assert!((position.seq, position.char_offset) > previous);
        let page = store
            .read_page(&active, Some(position), MIN_AGENT_PAGE_CHARS, None)
            .unwrap();
        assert_eq!(page.text.chars().count(), MIN_AGENT_PAGE_CHARS);
        assert_eq!(
            page.through_seq,
            page.next.map_or(6, |position| position.seq - 1)
        );
        glyphs.extend(
            page.text
                .chars()
                .filter(|ch| matches!(ch, 'æ' | '🦉' | '漢')),
        );
        previous = (position.seq, position.char_offset);
        next = page.next;
        if next.is_none() {
            break;
        }
    }
    assert_eq!(glyphs, "æ🦉漢");
    assert_eq!(next, None);
    assert!(store
        .read_page(&active, None, MIN_AGENT_PAGE_CHARS - 1, None)
        .is_err());
}

#[test]
fn routed_actions_are_idempotent_and_bound_to_the_exact_epoch_and_committed_range() {
    let (temp, store) = fixture(DEFAULT_RAW_BUDGET_BYTES);
    let original = attach(&store, epoch("one", "router-1"));
    append(&store, "route this");
    let action = RoutedAction {
        action_id: "action-1".into(),
        range: TranscriptRange {
            from_seq: 1,
            to_seq: 1,
        },
        at_ms: 420,
        destination: "router-1".into(),
        rationale: "requested next step".into(),
    };
    let record = store.record_action(&original, action.clone()).unwrap();
    assert_eq!(
        store.record_action(&original, action.clone()).unwrap(),
        record
    );
    assert_eq!(record.action_seq, 1);
    let mut outside = action.clone();
    outside.range.to_seq = 2;
    assert!(store.record_action(&original, outside).is_err());
    let new = attach(&store, epoch("two", "router-2"));
    assert!(store.record_action(&original, action.clone()).is_err());
    assert!(store.record_action(&new, action.clone()).is_err());
    let mut second = action.clone();
    second.action_id = "another-action".into();
    second.destination = "router-2".into();
    let second = store.record_action(&new, second).unwrap();
    assert_eq!(second.action_seq, 2);
    drop(store);

    let store = TranscriptStore::open(temp.path(), OWNER, DEFAULT_RAW_BUDGET_BYTES).unwrap();
    assert_eq!(
        store.routed_action(&action.action_id).unwrap(),
        Some(record.clone())
    );
    let page = store.routed_actions(0, 1).unwrap();
    assert_eq!(page.actions, vec![record]);
    assert_eq!(page.next_after_seq, Some(1));
    let page = store
        .routed_actions(page.next_after_seq.unwrap(), 1)
        .unwrap();
    assert_eq!(page.actions, vec![second]);
    assert_eq!(page.next_after_seq, None);
    assert_eq!(
        fs::read_dir(root(temp.path(), OWNER).join("route_actions"))
            .unwrap()
            .count(),
        2
    );
}

#[test]
fn exact_close_reopen_and_retirement_respect_readers_and_owner_scope() {
    let (temp, store) = fixture(0);
    let foreign = TranscriptStore::open(temp.path(), "foreign", 0).unwrap();
    foreign.append("foreign", speech("foreign data")).unwrap();
    let binding = attach(&store, epoch("one", "router-1"));
    append(&store, "can evict");
    let safe = store.seal_active(OWNER).unwrap().unwrap();
    store.ack(&binding, 1).unwrap();
    summarize(&store, safe, "kept summary");
    append(&store, "owner recovery unread");
    let first_close = closed(OWNER, "group-1", 100);
    let wrong_group = closed(OWNER, "foreign-group", 100);
    assert!(store
        .close_owner(OWNER, closed("foreign", "group-1", 100))
        .is_err());
    assert!(store.collect_retired(&first_close).is_err());

    let unread = store
        .close_owner(OWNER, first_close.clone())
        .unwrap()
        .unwrap();
    summarize(&store, unread, "unread summary");
    assert!(store.retire_owner(&wrong_group).is_err());
    assert!(store.reopen_owner(&wrong_group, "new-tab").is_err());
    assert!(store.collect_retired(&first_close).is_err());
    assert!(store.evict_to_budget().is_err());
    drop(store);

    let store = TranscriptStore::open(temp.path(), OWNER, 0).unwrap();
    store.reopen_owner(&first_close, "new-tab").unwrap();
    store.reopen_owner(&first_close, "new-tab").unwrap();
    assert!(store.reopen_owner(&first_close, "foreign").is_err());
    assert!(store.ack(&binding, 2).is_err());
    assert!(store.append(OWNER, speech("stale owner")).is_err());
    assert_eq!(
        store
            .append("new-tab", speech("retained lineage"))
            .unwrap()
            .row
            .seq,
        3
    );
    let binding = attach(&store, epoch("two", "router-2"));
    assert_eq!(store.cursor(&binding).unwrap().acked_seq, 1);
    let second_close = closed("new-tab", "group-2", 200);
    store.close_owner("new-tab", second_close.clone()).unwrap();
    assert!(store.retire_owner(&first_close).is_err());
    let reader = store.pin_reader().unwrap();
    assert_eq!(reader.store().history(1, 10).unwrap().rows.len(), 3);
    store.retire_owner(&second_close).unwrap();
    assert_eq!(
        store.collect_retired(&second_close).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    drop(reader);
    assert_eq!(store.collect_retired(&second_close).unwrap(), vec![safe]);
    assert!(store.reopen_owner(&second_close, "later-tab").is_err());
    assert_eq!(store.summary(safe).unwrap().unwrap().text, "kept summary");
    assert_eq!(store.history(1, 10).unwrap().rows.len(), 2);
    assert_eq!(
        foreign.history(1, 10).unwrap().rows[0].content,
        speech("foreign data").content
    );
}

#[test]
fn canonical_windows_safe_paths_refuse_aliases_and_foreign_files() {
    let (temp, store) = fixture(DEFAULT_RAW_BUDGET_BYTES);
    for invalid in [
        "../escape",
        "owner-1.",
        "OWNER-1",
        "owner:stream",
        "owner\\escape",
        "com1",
        "nul",
    ] {
        assert_eq!(
            TranscriptStore::open(temp.path(), invalid, 0)
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(store
            .set_binding(
                store.binding().revision,
                external(invalid),
                CursorStart::Beginning
            )
            .is_err());
    }
    let contender = TranscriptStore::open(temp.path(), OWNER, 0);
    assert_eq!(contender.err().unwrap().kind(), io::ErrorKind::WouldBlock);
    let foreign = TranscriptStore::open(temp.path(), "foreign", DEFAULT_RAW_BUDGET_BYTES).unwrap();
    append(&store, "owner prefix");
    foreign.append("foreign", speech("foreign prefix")).unwrap();
    drop(store);
    drop(foreign);
    let owner_raw = raw_path(&root(temp.path(), OWNER), 1);
    let foreign_raw = raw_path(&root(temp.path(), "foreign"), 1);
    let before = fs::read(&foreign_raw).unwrap();
    fs::remove_file(&owner_raw).unwrap();
    fs::hard_link(&foreign_raw, &owner_raw).unwrap();
    assert!(TranscriptStore::open(temp.path(), OWNER, 0).is_err());
    assert_eq!(fs::read(foreign_raw).unwrap(), before);
}
