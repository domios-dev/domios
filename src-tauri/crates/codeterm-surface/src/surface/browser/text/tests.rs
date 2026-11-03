use super::*;

#[test]
fn text_options_preserve_literals_and_bound_output() {
    let options =
        parse(&["--grep", "a'b", "200", "--selector", "main > article"].map(String::from)).unwrap();
    assert_eq!(options.grep.as_deref(), Some("a'b"));
    assert_eq!(options.selector.as_deref(), Some("main > article"));
    assert_eq!(options.limit, 200);
    for args in [
        vec!["0"],
        vec!["200001"],
        vec!["--grep"],
        vec!["--selector", ""],
        vec!["--grep", "x", "--grep", "y"],
    ] {
        assert!(parse(&args.into_iter().map(String::from).collect::<Vec<_>>()).is_err());
    }
}
