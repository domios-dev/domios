use super::*;
use crate::agent::{TabTodoChecklistStatus, TabTodoCounters};

fn entry(content: &str, state: TabTodoState) -> TabTodoWriteEntry {
    TabTodoWriteEntry {
        id: None,
        content: content.into(),
        state,
        link: None,
        reason: None,
    }
}

fn item(id: &str, title: &str, state: TabTodoState, locked: bool) -> TabTodoItem {
    TabTodoItem {
        id: id.into(),
        position: 0,
        title: title.into(),
        description: None,
        state,
        revision: 3,
        link: None,
        locked,
        reason: None,
        attempt: 0,
        counters: TabTodoCounters::default(),
        history: Vec::new(),
    }
}

fn checklist(items: Vec<TabTodoItem>) -> TabTodoChecklist {
    TabTodoChecklist {
        proposal: None,
        tab_id: "tab".into(),
        generation: 2,
        started_at: 1,
        revision: 9,
        status: TabTodoChecklistStatus::Active,
        items,
    }
}

fn existing(item_id: &str) -> TabTodoWriteTarget {
    TabTodoWriteTarget::Existing(TabTodoItemRef {
        item_id: item_id.into(),
        expected_revision: 3,
    })
}

#[test]
fn a_first_write_creates_the_whole_list_and_promotes_after_demotions() {
    let plan = plan_tab_todo_write(
        None,
        &[
            entry("Plan", TabTodoState::Done),
            entry("Build", TabTodoState::InProgress),
            entry("Verify", TabTodoState::Open),
        ],
    )
    .unwrap();
    assert_eq!(plan.base, TabTodoWriteBase::Fresh);
    let titles: Vec<_> = plan
        .appends
        .iter()
        .map(|entry| entry.content.as_str())
        .collect();
    assert_eq!(titles, ["Plan", "Build", "Verify"]);
    let targets: Vec<_> = plan
        .state_changes
        .iter()
        .map(|change| change.target.clone())
        .collect();
    assert_eq!(
        targets,
        [
            TabTodoWriteTarget::Appended(0),
            TabTodoWriteTarget::Appended(1)
        ]
    );
    assert!(plan_tab_todo_write(None, &[]).is_err());
}

#[test]
fn a_cleared_checklist_counts_as_absent() {
    let mut cleared = checklist(Vec::new());
    cleared.status = TabTodoChecklistStatus::Cleared;
    let plan = plan_tab_todo_write(Some(&cleared), &[entry("Again", TabTodoState::Open)]).unwrap();
    assert_eq!(plan.base, TabTodoWriteBase::Fresh);
}

#[test]
fn a_rewrite_keeps_matching_items_appends_new_ones_and_removes_omitted_unlocked_ones() {
    let current = checklist(vec![
        item("a", "Investigate", TabTodoState::InProgress, false),
        item("b", "Draft", TabTodoState::Open, false),
    ]);
    let plan = plan_tab_todo_write(
        Some(&current),
        &[
            entry("Investigate", TabTodoState::Done),
            entry("Implement", TabTodoState::InProgress),
        ],
    )
    .unwrap();
    assert_eq!(plan.base, TabTodoWriteBase::Edit);
    assert_eq!(
        plan.removals,
        [TabTodoItemRef {
            item_id: "b".into(),
            expected_revision: 3
        }]
    );
    assert_eq!(plan.appends.len(), 1);
    assert_eq!(plan.appends[0].content, "Implement");
    let order: Vec<_> = plan
        .state_changes
        .iter()
        .map(|change| (change.target.clone(), change.state))
        .collect();
    assert_eq!(
        order,
        [
            (existing("a"), TabTodoState::Done),
            (TabTodoWriteTarget::Appended(0), TabTodoState::InProgress),
        ]
    );
}

#[test]
fn omitted_locked_items_stay_and_count_toward_the_in_progress_rule() {
    let current = checklist(vec![
        item("seeded", "Acceptance one", TabTodoState::InProgress, true),
        item("mine", "Scratch", TabTodoState::Open, false),
    ]);
    let plan = plan_tab_todo_write(Some(&current), &[entry("Extra", TabTodoState::Open)]).unwrap();
    assert_eq!(plan.base, TabTodoWriteBase::Edit);
    assert_eq!(plan.omitted_locked_items, 1);
    assert_eq!(plan.removals.len(), 1);
    assert_eq!(plan.removals[0].item_id, "mine");

    let error = plan_tab_todo_write(Some(&current), &[entry("Extra", TabTodoState::InProgress)])
        .unwrap_err();
    assert!(
        error.contains("Acceptance one") && error.contains("Extra"),
        "{error}"
    );
}

#[test]
fn the_in_progress_refusal_names_every_offending_item() {
    let error = plan_tab_todo_write(
        None,
        &[
            entry("First", TabTodoState::InProgress),
            entry("Second", TabTodoState::InProgress),
        ],
    )
    .unwrap_err();
    assert!(
        error.contains("First") && error.contains("Second"),
        "{error}"
    );
}

#[test]
fn an_unchanged_declaration_is_a_noop_and_a_new_reason_is_a_change() {
    let mut blocked = item("a", "Wait", TabTodoState::Blocked, true);
    blocked.reason = Some("owner decision".into());
    let current = checklist(vec![blocked]);
    let mut same = entry("Wait", TabTodoState::Blocked);
    same.reason = Some("owner decision".into());
    assert!(plan_tab_todo_write(Some(&current), &[same.clone()])
        .unwrap()
        .is_noop());
    same.reason = Some("dependency moved".into());
    let plan = plan_tab_todo_write(Some(&current), &[same]).unwrap();
    assert_eq!(plan.state_changes.len(), 1);
}

#[test]
fn removing_every_unlocked_item_clears_or_starts_a_new_generation() {
    let current = checklist(vec![item("a", "Old", TabTodoState::Done, false)]);
    let cleared = plan_tab_todo_write(Some(&current), &[]).unwrap();
    assert_eq!(cleared.base, TabTodoWriteBase::Clear);
    let rewritten =
        plan_tab_todo_write(Some(&current), &[entry("New", TabTodoState::Open)]).unwrap();
    assert_eq!(rewritten.base, TabTodoWriteBase::Fresh);
}

#[test]
fn bounds_duplicates_reasons_and_locked_links_are_refused() {
    let many: Vec<_> = (0..=MAX_TAB_TODO_ITEMS)
        .map(|index| entry(&format!("Step {index}"), TabTodoState::Open))
        .collect();
    assert_eq!(
        plan_tab_todo_write(None, &many).unwrap_err(),
        tab_todo_bound_refusal()
    );

    let locked: Vec<_> = (0..MAX_TAB_TODO_ITEMS)
        .map(|index| {
            item(
                &format!("l{index}"),
                &format!("Locked {index}"),
                TabTodoState::Open,
                true,
            )
        })
        .collect();
    let full = checklist(locked);
    assert_eq!(
        plan_tab_todo_write(Some(&full), &[entry("One more", TabTodoState::Open)]).unwrap_err(),
        tab_todo_bound_refusal()
    );

    assert!(plan_tab_todo_write(
        None,
        &[
            entry("Twice", TabTodoState::Open),
            entry("Twice", TabTodoState::Done)
        ]
    )
    .is_err());
    assert!(plan_tab_todo_write(None, &[entry("Stuck", TabTodoState::Blocked)]).is_err());

    let mut linked = item("a", "Linked", TabTodoState::Open, true);
    linked.link = Some(TabTodoLink::Url {
        url: "https://example.test/a".into(),
        label: "Spec".into(),
    });
    let current = checklist(vec![linked]);
    let mut same_link = entry("Linked", TabTodoState::Done);
    same_link.link = Some("Spec".into());
    assert!(plan_tab_todo_write(Some(&current), &[same_link.clone()]).is_ok());
    same_link.link = Some("https://example.test/other".into());
    assert!(plan_tab_todo_write(Some(&current), &[same_link]).is_err());
}

#[test]
fn a_new_entry_link_becomes_a_url_or_a_task_reference() {
    let mut url = entry("Read spec", TabTodoState::Open);
    url.link = Some("https://example.test/spec".into());
    assert!(matches!(
        url.draft().link,
        Some(TabTodoLinkDraft::Url { .. })
    ));
    let mut task = entry("Follow task", TabTodoState::Open);
    task.link = Some("task_abc".into());
    assert!(matches!(
        task.draft().link,
        Some(TabTodoLinkDraft::DomiosTask { task_ref, .. }) if task_ref == "task_abc"
    ));
}

#[test]
fn id_write_changes_only_status_and_preserves_locked_items() {
    let current = checklist(vec![item(
        "seeded",
        "Long criterion",
        TabTodoState::Open,
        true,
    )]);
    let mut update = entry("", TabTodoState::Done);
    update.id = Some("seeded".into());
    let plan = plan_tab_todo_write(Some(&current), &[update.clone()]).unwrap();
    assert!(plan.appends.is_empty() && plan.removals.is_empty());
    assert_eq!(plan.state_changes[0].target, existing("seeded"));
    assert_eq!(plan.state_changes[0].state, TabTodoState::Done);
    update.content = "Changed title".into();
    assert!(plan_tab_todo_write(Some(&current), &[update.clone()])
        .unwrap_err()
        .contains("retitle"));
    update.id = Some("unknown".into());
    assert!(plan_tab_todo_write(Some(&current), &[update]).is_err());
    let omitted = plan_tab_todo_write(Some(&current), &[]).unwrap();
    assert!(omitted.removals.is_empty());
    assert_eq!(omitted.omitted_locked_items, 1);
    let fallback = plan_tab_todo_write(
        Some(&current),
        &[entry("Long criterion", TabTodoState::Done)],
    )
    .unwrap();
    assert_eq!(fallback.state_changes[0].target, existing("seeded"));
}
