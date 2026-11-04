use super::*;

#[test]
fn submit_flag_never_becomes_inserted_text() {
    for args in [
        vec!["--submit", "textbox", "Search", "query"],
        vec!["textbox", "Search", "query", "--submit"],
    ] {
        let (words, submit) = parse(
            &args.into_iter().map(String::from).collect::<Vec<_>>(),
            false,
        )
        .unwrap();
        assert!(submit);
        assert_eq!(words, ["textbox", "Search", "query"]);
    }
    assert!(parse(&["--submit".into(), "@e1".into()], false).is_err());
    let (words, submit) =
        parse(&["textbox", "Password", "--submit"].map(String::from), true).unwrap();
    assert!(!submit);
    assert_eq!(words.last().unwrap(), "--submit");
    let (words, submit) = parse(&["@e1", "--submit", "secret"].map(String::from), true).unwrap();
    assert!(submit);
    assert_eq!(words, ["@e1", "secret"]);
}
