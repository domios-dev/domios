use super::*;
#[test]
fn browser_snapshot_ref_depth_validation_and_pruning() {
    let args = |words: &[&str]| words.iter().map(|s| (*s).into()).collect::<Vec<String>>();
    assert_eq!(
        parse(&args(&["--ref", "@a", "--depth", "2"]))
            .unwrap()
            .depth,
        Some(2)
    );
    for words in [
        vec!["--ref"],
        vec!["--ref", "button"],
        vec!["--depth", "65"],
        vec!["--depth", "-1"],
        vec!["--depth"],
        vec!["--ref", "@a", "--scope", "dialog"],
    ] {
        assert!(parse(&args(&words)).is_err());
    }
    let nodes = parse_full_ax_tree(&serde_json::json!({"nodes":[
        {"nodeId":"root","childIds":["child"],"backendDOMNodeId":1},
        {"nodeId":"child","parentId":"root","childIds":["leaf"],"backendDOMNodeId":2},
        {"nodeId":"leaf","parentId":"child","backendDOMNodeId":3}
    ]}))
    .unwrap();
    let (subtree, truncated) = restrict(nodes.clone(), Some(2), Some(0)).unwrap();
    assert!(truncated);
    assert_eq!(subtree.len(), 1);
    assert!(subtree[0].parent_id.is_none() && subtree[0].child_ids.is_empty());
    let (subtree, truncated) = restrict(nodes.clone(), Some(2), None).unwrap();
    assert!(!truncated);
    assert_eq!(subtree.len(), 2);
    assert!(restrict(nodes, Some(99), None).is_err());
}
#[test]
fn bounded_snapshot_options_clamp_items_and_preserve_scope() {
    let args = [
        "--json",
        "--scope",
        "dialog:Settings",
        "--actions-only",
        "--max-items",
        "5",
    ]
    .map(String::from);
    assert_eq!(
        parse(&args).unwrap(),
        Options {
            json: true,
            scope: Some("dialog:Settings".into()),
            actions: true,
            limit: 5,
            with_text: false,
            reference: None,
            depth: None,
        }
    );
    assert_eq!(
        parse(&["--max-items".into(), "9000".into()]).unwrap().limit,
        MAX_LINES
    );
    assert!(parse(&["--max-items".into(), "0".into()]).is_err());
    assert!(parse(&["--scope".into()]).is_err());
}

#[test]
fn with_text_snapshot_options_can_be_combined_with_controls_only_and_bounds() {
    let options =
        parse(&["--with-text", "--actions-only", "--max-items", "7"].map(String::from)).unwrap();
    assert!(options.with_text && options.actions);
    assert_eq!(options.limit, 7);
}
