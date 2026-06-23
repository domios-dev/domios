use super::*;
use crate::plan::OrchestratorRef;
use crate::plan::Plan;

#[test]
fn store_saves_loads_indexes() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "T".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("abc"),
    );
    store.save(&mut p, 1000).unwrap();
    let path = dir
        .path()
        .join(".codeterm/plans")
        .join(format!("{}.json", p.id));
    assert!(path.exists());
    let loaded = store.get(&p.id).unwrap();
    assert_eq!(loaded.title, "T");
    assert!(store.list_index().iter().any(|e| e.plan_id == p.id));
    assert_eq!(loaded.updated_at, 1000);
}
