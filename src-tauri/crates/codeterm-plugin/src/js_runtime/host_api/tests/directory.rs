use super::*;

#[test]
fn walk_refuses_a_wide_tree_at_the_entry_budget() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let over = WALK_ENTRY_BUDGET + 25;
    for i in 0..over {
        std::fs::write(root.join(format!("f{i}.jsonl")), b"x").unwrap();
    }

    let error = crate::js_runtime::vm::call_json::<serde_json::Value>(
        &vm_with(r#"module.exports={walk(path){return host.io.run(host.io.fs.walkDir(path,8));}}"#),
        "walk",
        &[json!(root)],
    )
    .unwrap_err();
    assert!(
        matches!(error, crate::js_runtime::vm::VmError::Script(message) if message.contains("directory result limit"))
    );
}

/// Under the budget nothing changes: every entry is still reported, so bounding
/// the walk did not quietly shrink an ordinary result.
#[test]
fn walk_under_the_budget_reports_every_entry() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::create_dir(root.join("nested")).unwrap();
    std::fs::write(root.join("a.jsonl"), b"x").unwrap();
    std::fs::write(root.join("nested").join("b.jsonl"), b"x").unwrap();

    let walked: Vec<serde_json::Value> = crate::js_runtime::vm::call_json(
        &vm_with(r#"module.exports={walk(path){return host.io.run(host.io.fs.walkDir(path,8));}}"#),
        "walk",
        &[json!(root)],
    )
    .unwrap();
    let names: Vec<&str> = walked
        .iter()
        .filter_map(|entry| entry.get("name").and_then(|n| n.as_str()))
        .collect();
    assert_eq!(walked.len(), 3, "two files and one directory");
    for expected in ["a.jsonl", "nested", "b.jsonl"] {
        assert!(
            names.contains(&expected),
            "{expected} missing from {names:?}"
        );
    }
}
