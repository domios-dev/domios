use super::*;
use crate::{MemKind, MemManager, MemScope, NewMemItem};
use codeterm_blueprint_types::blueprint_memory::CuratorEvidence;

#[test]
fn stale_low_solution_is_proposed_until_reobserved() {
    let mem = MemManager::open_in_memory().unwrap();
    let item = || NewMemItem {
        kind: MemKind::Skill,
        scope: MemScope::Global,
        title: "Watch CI".into(),
        summary: None,
        area: None,
        tags: vec![],
        body: "General instructions".into(),
        source: None,
        importance: 0.5,
        source_path: None,
        pinned: false,
    };
    let proof = |round| CuratorEvidence {
        plan: "pl_test".into(),
        round,
        episodes: vec!["task:a".into(), "task:b".into()],
        granted: false,
    };
    let id = mem.save_curator_skill(item(), None, proof(1), 100).unwrap();
    let threshold = 100 + 60 * 86400;
    assert!(render(&mem.fixture_conn().lock(), threshold - 1)
        .unwrap()
        .is_none());
    assert!(render(&mem.fixture_conn().lock(), threshold)
        .unwrap()
        .unwrap()
        .contains(&id));
    mem.save_curator_skill(item(), Some(&id), proof(2), threshold)
        .unwrap();
    assert!(render(&mem.fixture_conn().lock(), threshold + 90 * 86400)
        .unwrap()
        .is_none());
}
