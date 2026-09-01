use super::*;
use crate::store::{MemItem, MemUpdate};

fn find(mem: &MemManager, title: &str) -> MemItem {
    mem.search(&SearchQuery {
        query: title.into(),
        limit: Some(5),
        ..Default::default()
    })
    .unwrap()
    .into_iter()
    .find(|hit| hit.item.title == title)
    .unwrap()
    .item
}

fn fingerprint_of(mem: &MemManager, id: &str) -> Option<String> {
    mem.managed_payload(id).unwrap().unwrap().1
}

/// Same as `find`, but disambiguates when two rows share a title (a
/// kind-drifted fixture plus the freshly created canonical one).
fn find_excluding(mem: &MemManager, title: &str, exclude_id: &str) -> MemItem {
    mem.search(&SearchQuery {
        query: title.into(),
        limit: Some(5),
        ..Default::default()
    })
    .unwrap()
    .into_iter()
    .map(|hit| hit.item)
    .find(|item| item.title == title && item.id != exclude_id)
    .unwrap()
}

fn baseline() -> NewMemItem {
    NewMemItem {
        kind: MemKind::Skill,
        scope: MemScope::Global,
        title: "codeterm".into(),
        summary: Some("summary".into()),
        area: Some("area".into()),
        tags: vec!["a".into(), "b".into()],
        body: "body".into(),
        source: Some("bundled:v2".into()),
        importance: 4.0,
        source_path: Some("path".into()),
        pinned: true,
    }
}

fn codeterm_item() -> NewMemItem {
    bundled_items().into_iter().nth(4).unwrap()
}

fn versioned_codeterm(body: &str) -> NewMemItem {
    let mut item = codeterm_item();
    item.body = body.into();
    item
}

#[test]
fn seed_runs_once() {
    let mem = MemManager::open_in_memory().unwrap();
    let first = seed_bundled(&mem).unwrap();
    assert!(first.written() >= 5, "expected >=5 seeded items, got {}", first.written());
    assert!(first.skipped.is_empty());

    let second = seed_bundled(&mem).unwrap();
    assert_eq!(second.written(), 0, "second seed should be a true no-op");
    assert!(second.skipped.is_empty());
}

/// DRIFT GUARD: two payloads differing in exactly one managed field produce
/// different fingerprints, parameterised over every field in the set.
#[test]
fn fingerprint_differs_for_each_managed_field() {
    let base = baseline();
    let base_fp = seed_fingerprint(&base);

    let mutations: [(&str, fn(&mut NewMemItem)); 9] = [
        ("kind", |i| i.kind = MemKind::Rule),
        ("scope", |i| i.scope = MemScope::Workspace("w1".into())),
        ("title", |i| i.title = "different".into()),
        ("summary", |i| i.summary = Some("different".into())),
        ("area", |i| i.area = Some("different".into())),
        ("tags", |i| i.tags.push("extra".into())),
        ("body", |i| i.body = "different body".into()),
        ("importance", |i| i.importance = 5.0),
        ("source_path", |i| i.source_path = Some("different".into())),
    ];
    for (field, mutate) in mutations {
        let mut mutated = base.clone();
        mutate(&mut mutated);
        assert_ne!(
            seed_fingerprint(&mutated),
            base_fp,
            "changing only {field} must change the fingerprint"
        );
    }

    let mut pinned_flip = base.clone();
    pinned_flip.pinned = !base.pinned;
    assert_ne!(seed_fingerprint(&pinned_flip), base_fp, "changing only pinned must change the fingerprint");
}

/// `source` is the provenance marker, not managed content: it is excluded.
#[test]
fn fingerprint_excludes_source() {
    let mut a = baseline();
    let mut b = baseline();
    a.source = Some("bundled:v2".into());
    b.source = Some("user".into());
    assert_eq!(seed_fingerprint(&a), seed_fingerprint(&b));
}

/// COLLISION GUARD: a naive joined/delimited encoding would confuse these
/// pairs across a field boundary; the JSON-array encoding must not.
#[test]
fn fingerprint_is_collision_safe_across_field_boundaries() {
    let mut a = baseline();
    a.body = "X".into();
    a.source_path = Some("Y".into());
    let mut b = baseline();
    b.body = "XY".into();
    b.source_path = Some("".into());
    assert_ne!(seed_fingerprint(&a), seed_fingerprint(&b));

    let mut c = baseline();
    c.tags = vec!["a,b".into()];
    let mut d = baseline();
    d.tags = vec!["a".into(), "b".into()];
    assert_ne!(seed_fingerprint(&c), seed_fingerprint(&d));
}

/// ATOMIC CREATION: `save_seeded` records payload and fingerprint in the
/// same insert — never a fresh item observable without one.
#[test]
fn fresh_creation_is_fingerprinted_in_the_same_write() {
    let mem = MemManager::open_in_memory().unwrap();
    let item = codeterm_item();
    let fp = seed_fingerprint(&item);

    let id = mem.save_seeded(item, &fp).unwrap();

    assert_eq!(fingerprint_of(&mem, &id), Some(fp));
}

/// BOOTSTRAP ATOMICITY: an edit landing between the read and the bootstrap
/// attempt is refused — `IS NULL` alone would accept it too (edits clear it).
#[test]
fn bootstrap_refuses_an_edit_between_read_and_write() {
    let mem = MemManager::open_in_memory().unwrap();
    let shipped = codeterm_item();
    let id = mem.save(shipped.clone()).unwrap();
    let stale_expected = shipped.clone(); // what the seeder "read" before the race

    mem.update(
        &id,
        MemUpdate {
            body: Some("a concurrent user edit".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(fingerprint_of(&mem, &id), None, "the edit clears the fingerprint too");

    let applied = mem
        .bootstrap_seed_fingerprint(&id, &stale_expected, &seed_fingerprint(&shipped))
        .unwrap();

    assert!(!applied, "bootstrap must not accept a payload it didn't just verify");
    assert_eq!(fingerprint_of(&mem, &id), None);
    assert_eq!(find(&mem, "codeterm").body, "a concurrent user edit");
}

/// An unrecognized `scope` value must be treated as unreadable, never
/// silently coerced to Global by `MemScope::from_str`'s fallback.
#[test]
fn unknown_scope_is_unreadable() {
    let mem = MemManager::open_in_memory().unwrap();
    let shipped = codeterm_item();
    let id = mem.save(shipped.clone()).unwrap();
    mem.with_conn(|conn| {
        conn.execute(
            "UPDATE mem_items SET scope = 'not-a-real-scope' WHERE id = ?1",
            rusqlite::params![&id],
        )
    })
    .unwrap();

    let outcome = seed_from(&mem, vec![shipped]).unwrap();

    assert_eq!(outcome.written(), 0);
    assert_eq!(
        outcome.skipped,
        vec![("codeterm".to_string(), SeedSkipReason::Unreadable)]
    );
}

/// BOOTSTRAP, exact match: a pre-fingerprint item exactly matching today's
/// shipped payload gains a fingerprint and nothing else changes.
#[test]
fn bootstrap_gains_fingerprint_when_it_matches_todays_payload() {
    let mem = MemManager::open_in_memory().unwrap();
    let shipped = codeterm_item();
    let id = mem.save(shipped.clone()).unwrap();
    assert_eq!(fingerprint_of(&mem, &id), None);
    let before = find(&mem, "codeterm");

    let outcome = seed_from(&mem, vec![shipped.clone()]).unwrap();

    assert_eq!(outcome.bootstrapped, 1);
    assert_eq!(outcome.written(), 1);
    assert!(outcome.skipped.is_empty());
    let after = find(&mem, "codeterm");
    assert_eq!(after.body, before.body);
    assert_eq!(after.summary, before.summary);
    assert_eq!(after.tags, before.tags);
    assert_eq!(after.importance, before.importance);
    assert_eq!(fingerprint_of(&mem, &id), Some(seed_fingerprint(&shipped)));
}

/// NEGATIVE: a pre-fingerprint item whose payload does NOT exactly match
/// today's shipped one (summary differs, body unchanged) is not overwritten.
#[test]
fn ambiguous_legacy_item_is_preserved() {
    let mem = MemManager::open_in_memory().unwrap();
    let mut stored = codeterm_item();
    stored.summary = Some("a summary nobody shipped".into());
    let id = mem.save(stored.clone()).unwrap();

    let outcome = seed_from(&mem, vec![codeterm_item()]).unwrap();

    assert_eq!(outcome.written(), 0);
    assert_eq!(outcome.skipped, vec![("codeterm".to_string(), SeedSkipReason::Absent)]);
    assert_eq!(fingerprint_of(&mem, &id), None);
    let after = find(&mem, "codeterm");
    assert_eq!(after.body, stored.body);
    assert_eq!(after.summary, stored.summary);
}

fn legacy_1_10_7_codeterm() -> NewMemItem {
    legacy_1_10_7_items()
        .into_iter()
        .find(|i| i.title == "codeterm")
        .unwrap()
}

/// codeterm#321 END TO END, ONE PASS: a REAL 1.10.7 row reaches current
/// content in the SAME `seed_bundled` pass — bootstrap chained into refresh.
#[test]
fn real_1_10_7_row_upgrades_in_one_pass() {
    let mem = MemManager::open_in_memory().unwrap();
    let legacy = legacy_1_10_7_codeterm();
    let id = mem.save(legacy.clone()).unwrap();
    assert_eq!(fingerprint_of(&mem, &id), None, "a real 1.10.7 row predates fingerprints");

    // Stand-in for "today's shipped payload has since diverged from 1.10.7"
    // (a future prompt edit) — the bridge, not the current-payload fast
    // path, must be what recognizes this row as unedited.
    let current = versioned_codeterm("hypothetical v3 content, post-1.10.7");
    let outcome = seed_from(&mem, vec![current.clone()]).unwrap();

    assert_eq!(outcome.bootstrapped, 1, "bootstrapped against the legacy payload");
    assert_eq!(outcome.refreshed, 1, "chained into the CAS refresh in the same pass");
    assert_eq!(outcome.written(), 2);
    assert!(outcome.skipped.is_empty());
    let after = find(&mem, "codeterm");
    assert_eq!(after.body, current.body, "already on current content when this call returns");
    assert_ne!(after.body, legacy.body);
    assert_eq!(fingerprint_of(&mem, &id), Some(seed_fingerprint(&current)));
}

/// A body-edited variant of the real 1.10.7 payload does not upgrade.
#[test]
fn body_edited_1_10_7_variant_does_not_bootstrap() {
    let mem = MemManager::open_in_memory().unwrap();
    let mut edited = legacy_1_10_7_codeterm();
    edited.body = format!("{}\nuser-authored addition", edited.body);
    let id = mem.save(edited.clone()).unwrap();

    let outcome = seed_from(&mem, vec![versioned_codeterm("hypothetical v3 content")]).unwrap();

    assert_eq!(outcome.written(), 0);
    assert_eq!(outcome.skipped, vec![("codeterm".to_string(), SeedSkipReason::Absent)]);
    assert_eq!(fingerprint_of(&mem, &id), None);
    assert_eq!(find(&mem, "codeterm").body, edited.body);
}

/// A metadata-only-edited variant of the real 1.10.7 payload — body
/// byte-identical, summary differs — also does not upgrade.
#[test]
fn metadata_edited_1_10_7_variant_does_not_bootstrap() {
    let mem = MemManager::open_in_memory().unwrap();
    let mut edited = legacy_1_10_7_codeterm();
    edited.summary = Some("a user's own summary".into());
    let id = mem.save(edited.clone()).unwrap();

    let outcome = seed_from(&mem, vec![versioned_codeterm("hypothetical v3 content")]).unwrap();

    assert_eq!(outcome.written(), 0);
    assert_eq!(outcome.skipped, vec![("codeterm".to_string(), SeedSkipReason::Absent)]);
    let after = find(&mem, "codeterm");
    assert_eq!(after.summary, edited.summary);
    assert_eq!(after.body, edited.body);
}

/// Every single-field drift fails closed. Title/kind get their own tests
/// below — the search is keyed on both, so drifting either creates anew.
#[test]
fn every_single_field_drift_from_1_10_7_fails_closed() {
    let mutations: [(&str, fn(&mut NewMemItem)); 7] = [
        ("scope", |i| i.scope = MemScope::Workspace("w1".into())),
        ("summary", |i| i.summary = Some("drifted".into())),
        ("area", |i| i.area = Some("drifted".into())),
        ("tags", |i| i.tags.push("extra".into())),
        ("body", |i| i.body.push_str("\ndrift")),
        ("importance", |i| i.importance += 0.1),
        ("source_path", |i| i.source_path = Some("drifted".into())),
    ];
    for (field, mutate) in mutations {
        let mem = MemManager::open_in_memory().unwrap();
        let mut drifted = legacy_1_10_7_codeterm();
        mutate(&mut drifted);
        let id = mem.save(drifted.clone()).unwrap();

        let outcome =
            seed_from(&mem, vec![versioned_codeterm("hypothetical v3 content")]).unwrap();

        assert_eq!(outcome.written(), 0, "{field} drift must not upgrade");
        assert_eq!(fingerprint_of(&mem, &id), None, "{field} drift must not fingerprint");
    }

    let mem = MemManager::open_in_memory().unwrap();
    let mut pinned_drift = legacy_1_10_7_codeterm();
    pinned_drift.pinned = !pinned_drift.pinned;
    let id = mem.save(pinned_drift).unwrap();
    seed_from(&mem, vec![versioned_codeterm("hypothetical v3 content")]).unwrap();
    assert_eq!(fingerprint_of(&mem, &id), None, "pinned drift must not fingerprint");
}

/// A `title` drift: never found (title-keyed search), survives untouched,
/// no fingerprint — a SEPARATE canonical item is created, never adopting it.
#[test]
fn title_drifted_1_10_7_variant_is_untouched_and_a_new_item_is_created() {
    let mem = MemManager::open_in_memory().unwrap();
    let mut drifted = legacy_1_10_7_codeterm();
    drifted.title = "codeterm-renamed".into();
    let edited_id = mem.save(drifted.clone()).unwrap();

    let target = versioned_codeterm("hypothetical v3 content");
    let outcome = seed_from(&mem, vec![target.clone()]).unwrap();

    assert_eq!(outcome.created, 1, "the canonical title is unseeded, so it is created");
    assert_eq!(outcome.written(), 1);
    assert!(outcome.skipped.is_empty());

    // The edited row: untouched, no fingerprint, no ownership claimed.
    assert_eq!(fingerprint_of(&mem, &edited_id), None);
    let edited_row = mem.managed_payload(&edited_id).unwrap().unwrap().0;
    assert_eq!(edited_row.title, "codeterm-renamed");
    assert_eq!(edited_row.body, drifted.body);

    // The new row: a distinct id, current content, its own fingerprint.
    let created_row = find(&mem, "codeterm");
    assert_ne!(created_row.id, edited_id);
    assert_eq!(created_row.body, target.body);
    assert_eq!(fingerprint_of(&mem, &created_row.id), Some(seed_fingerprint(&target)));
}

/// A `kind` drift: excluded by the search's kind filter the same way — the
/// edited row survives untouched, a separate canonical item is created.
#[test]
fn kind_drifted_1_10_7_variant_is_untouched_and_a_new_item_is_created() {
    let mem = MemManager::open_in_memory().unwrap();
    let mut drifted = legacy_1_10_7_codeterm();
    drifted.kind = MemKind::Rule;
    let edited_id = mem.save(drifted.clone()).unwrap();

    let target = versioned_codeterm("hypothetical v3 content");
    let outcome = seed_from(&mem, vec![target.clone()]).unwrap();

    assert_eq!(outcome.created, 1, "the canonical kind is unseeded, so it is created");
    assert_eq!(outcome.written(), 1);
    assert!(outcome.skipped.is_empty());

    assert_eq!(fingerprint_of(&mem, &edited_id), None);
    let edited_row = mem.managed_payload(&edited_id).unwrap().unwrap().0;
    assert_eq!(edited_row.kind, MemKind::Rule);
    assert_eq!(edited_row.body, drifted.body);

    // Both rows now share the title "codeterm" (kind is what diverged), so
    // the canonical one must be found by excluding the edited row's id.
    let created_row = find_excluding(&mem, "codeterm", &edited_id);
    assert_ne!(created_row.id, edited_id);
    assert_eq!(created_row.body, target.body);
    assert_eq!(fingerprint_of(&mem, &created_row.id), Some(seed_fingerprint(&target)));
}

/// VERSION-AGNOSTIC (1/2): the generic mechanism refreshes a fingerprinted,
/// unmodified item across a v2-to-v3 content change.
#[test]
fn fingerprinted_v2_to_v3_refreshes() {
    let mem = MemManager::open_in_memory().unwrap();
    let v2 = versioned_codeterm("codeterm skill v2 content");
    seed_from(&mem, vec![v2]).unwrap();
    let id = find(&mem, "codeterm").id;
    let fp_v2 = fingerprint_of(&mem, &id);

    let v3 = versioned_codeterm("codeterm skill v3 content");
    let outcome = seed_from(&mem, vec![v3.clone()]).unwrap();

    assert_eq!(outcome.refreshed, 1);
    let after = find(&mem, "codeterm");
    assert_eq!(after.body, "codeterm skill v3 content");
    assert_ne!(fingerprint_of(&mem, &id), fp_v2);
    assert_eq!(fingerprint_of(&mem, &id), Some(seed_fingerprint(&v3)));
}

/// VERSION-AGNOSTIC (2/2): the identical mechanism, a v3-to-v4 fixture —
/// no version-specific branch, no new marker constant.
#[test]
fn fingerprinted_v3_to_v4_refreshes() {
    let mem = MemManager::open_in_memory().unwrap();
    let v3 = versioned_codeterm("codeterm skill v3 content");
    seed_from(&mem, vec![v3]).unwrap();
    let id = find(&mem, "codeterm").id;
    let fp_v3 = fingerprint_of(&mem, &id);

    let v4 = versioned_codeterm("codeterm skill v4 content");
    let outcome = seed_from(&mem, vec![v4.clone()]).unwrap();

    assert_eq!(outcome.refreshed, 1);
    let after = find(&mem, "codeterm");
    assert_eq!(after.body, "codeterm skill v4 content");
    assert_ne!(fingerprint_of(&mem, &id), fp_v3);
    assert_eq!(fingerprint_of(&mem, &id), Some(seed_fingerprint(&v4)));
}

/// OWNERSHIP RELINQUISH + user edit to body: the real `mem.update` path
/// clears the fingerprint atomically, so a later reseed preserves it.
#[test]
fn user_edit_to_body_is_preserved() {
    let mem = MemManager::open_in_memory().unwrap();
    let shipped = codeterm_item();
    let id = mem.save(shipped.clone()).unwrap();
    seed_from(&mem, vec![shipped.clone()]).unwrap();
    assert!(fingerprint_of(&mem, &id).is_some());

    mem.update(
        &id,
        MemUpdate {
            body: Some("user-authored codeterm skill body".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(fingerprint_of(&mem, &id), None, "a body edit must relinquish ownership");

    let outcome = seed_from(&mem, vec![shipped]).unwrap();

    assert_eq!(outcome.written(), 0);
    assert_eq!(find(&mem, "codeterm").body, "user-authored codeterm skill body");
}

/// A user edit to summary/tags/area/importance/pinned ALONE, body
/// byte-identical, must also be detected and preserved — not just body.
#[test]
fn user_edit_to_metadata_only_is_preserved() {
    let mem = MemManager::open_in_memory().unwrap();
    let shipped = codeterm_item();
    let id = mem.save(shipped.clone()).unwrap();
    seed_from(&mem, vec![shipped.clone()]).unwrap();
    assert!(fingerprint_of(&mem, &id).is_some());

    mem.update(
        &id,
        MemUpdate {
            tags: Some(vec!["user-tag".into()]),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(fingerprint_of(&mem, &id), None, "a tags-only edit must relinquish ownership");

    let outcome = seed_from(&mem, vec![shipped.clone()]).unwrap();

    assert_eq!(outcome.written(), 0);
    let after = find(&mem, "codeterm");
    assert_eq!(after.tags, vec!["user-tag".to_string()]);
    assert_eq!(after.body, shipped.body, "body was never touched by the metadata edit");
}

/// NEGATIVE: a fingerprint no longer matching the stored payload (changed
/// out-of-band, bypassing both write paths) is preserved, never overwritten.
#[test]
fn mismatched_fingerprint_is_preserved() {
    let mem = MemManager::open_in_memory().unwrap();
    let shipped = versioned_codeterm("a short body, guaranteed inline-stored");
    let id = mem.save(shipped.clone()).unwrap();
    seed_from(&mem, vec![shipped.clone()]).unwrap();
    let recorded = fingerprint_of(&mem, &id).unwrap();

    // Simulate out-of-band tampering: body changes without going through
    // `store::update`, so the fingerprint is left stale on purpose.
    mem.with_conn(|conn| {
        conn.execute(
            "UPDATE mem_items SET body_inline = ?1 WHERE id = ?2",
            rusqlite::params!["tampered body", &id],
        )
    })
    .unwrap();
    assert_eq!(fingerprint_of(&mem, &id), Some(recorded), "the stale fingerprint is still recorded");

    let outcome = seed_from(&mem, vec![shipped]).unwrap();

    assert_eq!(outcome.written(), 0);
    assert_eq!(
        outcome.skipped,
        vec![("codeterm".to_string(), SeedSkipReason::Mismatched)]
    );
    assert_eq!(find(&mem, "codeterm").body, "tampered body");
}

/// NEGATIVE, green before and after: an unreadable managed payload (here,
/// a kind the seeder no longer recognizes) is preserved, never overwritten.
#[test]
fn unreadable_payload_is_preserved() {
    let mem = MemManager::open_in_memory().unwrap();
    let shipped = codeterm_item();
    let id = mem.save(shipped.clone()).unwrap();
    mem.with_conn(|conn| {
        conn.execute(
            "UPDATE mem_items SET kind = 'not-a-real-kind' WHERE id = ?1",
            rusqlite::params![&id],
        )
    })
    .unwrap();

    let outcome = seed_from(&mem, vec![shipped]).unwrap();

    assert_eq!(outcome.written(), 0);
    assert_eq!(
        outcome.skipped,
        vec![("codeterm".to_string(), SeedSkipReason::Unreadable)]
    );
}

/// ATOMICITY oracle: a real edit committed after the seeder captured its
/// expected fingerprint must never be overwritten by a stale-targeted write.
#[test]
fn concurrent_edit_between_check_and_write_is_refused() {
    let mem = MemManager::open_in_memory().unwrap();
    let shipped = codeterm_item();
    let id = mem.save(shipped.clone()).unwrap();
    seed_from(&mem, vec![shipped.clone()]).unwrap();
    let stale_expected = fingerprint_of(&mem, &id).unwrap();

    // A real, independent edit lands between the seeder's read and its write.
    mem.update(
        &id,
        MemUpdate {
            body: Some("a concurrent user edit".into()),
            ..Default::default()
        },
    )
    .unwrap();

    let mut refreshed_item = shipped;
    refreshed_item.body = "codeterm skill v3 content".into();
    let applied = mem
        .cas_refresh_bundled(
            &id,
            Some(&stale_expected),
            &refreshed_item,
            &seed_fingerprint(&refreshed_item),
        )
        .unwrap();

    assert!(!applied, "a write against a stale fingerprint must not apply");
    assert_eq!(find(&mem, "codeterm").body, "a concurrent user edit");
}

#[test]
fn unchanged_reseed_is_a_no_op() {
    let mem = MemManager::open_in_memory().unwrap();
    seed_bundled(&mem).unwrap();

    let outcome = seed_bundled(&mem).unwrap();

    assert_eq!(outcome, SeedOutcome::default());
}

/// The relinquish happens in the SAME write as the edit — never a
/// follow-up step — so the item is never observably out of sync.
#[test]
fn ownership_relinquish_is_atomic_with_the_edit() {
    let mem = MemManager::open_in_memory().unwrap();
    let shipped = codeterm_item();
    let id = mem.save(shipped.clone()).unwrap();
    seed_from(&mem, vec![shipped]).unwrap();
    assert!(fingerprint_of(&mem, &id).is_some());

    mem.update(
        &id,
        MemUpdate {
            body: Some("edited".into()),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(fingerprint_of(&mem, &id), None);
}

/// A fresh install is fingerprinted at creation, so an immediate reseed
/// (no bundled content change yet) is already a no-op.
#[test]
fn fresh_install_is_fingerprinted_at_creation() {
    let mem = MemManager::open_in_memory().unwrap();

    let first = seed_bundled(&mem).unwrap();
    assert!(first.created >= 5);

    let id = find(&mem, "codeterm").id;
    assert!(fingerprint_of(&mem, &id).is_some());

    let second = seed_bundled(&mem).unwrap();
    assert_eq!(second.written(), 0);
}

#[test]
fn seeded_identity_appears_in_wake_up() {
    let mem = MemManager::open_in_memory().unwrap();
    seed_bundled(&mem).unwrap();
    let out = mem.wake_up(&crate::WakeUpOptions::default()).unwrap();
    assert!(out.contains("L0 — Identity"));
    assert!(out.contains("L1 — Critical context"));
    assert!(out.contains("never edit autogen") || out.contains("Rules"));
}
