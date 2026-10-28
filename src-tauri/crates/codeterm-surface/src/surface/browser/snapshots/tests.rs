use super::*;
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
            limit: 5
        }
    );
    assert_eq!(
        parse(&["--max-items".into(), "9000".into()]).unwrap().limit,
        MAX_LINES
    );
    assert!(parse(&["--max-items".into(), "0".into()]).is_err());
    assert!(parse(&["--scope".into()]).is_err());
}
