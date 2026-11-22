use super::*;

#[test]
fn shared_project_loads_worktrees_and_refs_once_across_lanes() {
    let mut cleanup = GitTeamLaneCleanup::new(&[]);
    let mut calls = Vec::new();
    for (project, branch) in [
        ("same", "lane/one"),
        ("same", "lane/two"),
        ("other", "lane/one"),
    ] {
        let inventory = cleanup
            .inventory_with(project, |project| {
                RepositoryInventory::load_with(project, |project, args| {
                    calls.push((project.to_string(), args[0].to_string()));
                    Ok(match args[0] {
                        "worktree" => "worktree /project\nHEAD abc\nbranch refs/heads/main\n",
                        "for-each-ref" => "refs/heads/lane/one abc\nrefs/heads/lane/two def\n",
                        _ => unreachable!(),
                    }
                    .to_string())
                })
            })
            .unwrap();
        assert!(inventory.refs.contains_key(&format!("refs/heads/{branch}")));
    }
    assert_eq!(
        calls,
        [
            ("same".to_string(), "worktree".to_string()),
            ("same".to_string(), "for-each-ref".to_string()),
            ("other".to_string(), "worktree".to_string()),
            ("other".to_string(), "for-each-ref".to_string()),
        ]
    );
}
