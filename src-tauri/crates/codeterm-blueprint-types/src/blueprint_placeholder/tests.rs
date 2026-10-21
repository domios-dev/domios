use super::*;

#[test]
fn one_grammar_preserves_command_templates_and_positions() {
    let text = "é {{ name }} {{repo.id}} {{.Status}} {{range .}} {{broken";
    let parsed = tokens(text);
    assert_eq!(
        parsed.iter().map(|token| token.key).collect::<Vec<_>>(),
        vec![Some("name"), Some("repo.id"), None, None, None]
    );
    assert_eq!(&text[parsed[0].span.clone()], "{{ name }}");
    assert!(!parsed.last().unwrap().closed);
    for key in ["", ".Status", "range .", "a b"] {
        assert!(!valid_key(key));
    }
}
