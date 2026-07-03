use super::*;

#[test]
fn titleize_from_slug() {
    assert_eq!(titleize("deploy-to-staging"), "Deploy To Staging");
    assert_eq!(titleize("single"), "Single");
    assert_eq!(titleize(""), "");
}

// End-to-end store+ledger flow mirroring new -> edit -> list, without going
// through cwd/git-based plan resolution.
#[test]
fn create_edit_list_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let plan_dir = tmp.path();

    // new
    let slug = store::slugify("Deploy To Staging");
    let dir = store::skill_dir(plan_dir, &slug);
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(
        store::skill_md_path(plan_dir, &slug),
        store::scaffold(&slug, "Deploy To Staging", "infra", None),
    )
    .unwrap();
    ledger::append_entry(plan_dir, &slug, store::KEEP, "created").unwrap();

    assert_eq!(store::list_skills(plan_dir), vec![slug.clone()]);
    assert_eq!(
        ledger::latest_verdict(plan_dir, &slug).as_deref(),
        Some("keep")
    );

    // edit -> supersede
    ledger::append_entry(plan_dir, &slug, store::SUPERSEDED, "replaced by v2").unwrap();
    assert_eq!(
        ledger::latest_verdict(plan_dir, &slug).as_deref(),
        Some("superseded")
    );
    // History is append-only: both entries survive.
    let history: Vec<_> = ledger::read_ledger(plan_dir)
        .into_iter()
        .filter(|e| e.slug == slug)
        .collect();
    assert_eq!(history.len(), 2);
}
