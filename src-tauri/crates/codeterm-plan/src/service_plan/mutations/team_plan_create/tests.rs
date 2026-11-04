use super::*;

/// codeterm#384: a bare/relative `--project` name must never be resolved
/// against the daemon's own process cwd — canonicalizing a relative path
/// resolves it against whatever directory the daemon process happens to
/// be running from (e.g. the CodeTerm data dir), not the caller's actual
/// checkout. The refusal below is filesystem-independent by construction
/// (checked before any path is touched), so it holds even for the exact
/// failing case: a bare name that ALSO exists as a directory under the
/// CodeTerm data dir — proven here by a bare name that indeed exists as a
/// directory relative to the current process cwd, which must still not
/// be enough to accept it.
#[test]
fn bare_project_name_is_refused_even_when_it_exists_under_the_process_cwd() {
    let cwd = std::env::current_dir().unwrap();
    let bare_name = "src"; // a real directory relative to this crate's own test cwd
    assert!(
        cwd.join(bare_name).is_dir(),
        "fixture precondition: {bare_name} must exist as a directory relative to cwd"
    );
    let store_dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(store_dir.path());
    let result = store.create_plan(
        "Mis-rooted".into(),
        bare_name.into(),
        OrchestratorRef::from_tab("orch"),
        vec![],
        1,
    );
    assert!(
        matches!(result, Err(PlanError::Io(_))),
        "a bare project name must be refused at creation, not recorded: {result:?}"
    );
}

#[test]
fn absolute_project_path_records_the_resolved_checkout() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let plan = store
        .create_plan(
            "Rooted".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
            vec![],
            1,
        )
        .expect("an absolute, existing checkout path must be accepted");
    assert_eq!(
        plan.project_path,
        paths::project_root(dir.path()).display().to_string()
    );
}
