use super::*;

#[test]
fn plain_scalars_resolve_by_the_yaml_core_schema() {
    for (text, kind) in [
        ("", PlainKind::Null),
        ("~", PlainKind::Null),
        ("True", PlainKind::Bool(true)),
        ("false", PlainKind::Bool(false)),
        ("12", PlainKind::Int),
        ("0x1f", PlainKind::Int),
        ("1.20", PlainKind::Float),
        ("-.5e3", PlainKind::Float),
        (".inf", PlainKind::Float),
        ("2.1.0", PlainKind::Str),
        ("45m..2h", PlainKind::Str),
        ("yes", PlainKind::Str),
    ] {
        assert_eq!(resolve_plain(text), kind, "{text}");
    }
}

#[test]
fn plain_style_is_used_only_when_it_reads_back_as_the_same_string() {
    for text in [
        "claude:opus",
        "https://example.com/a",
        "Apache-2.0",
        "2.1.0",
        "C# notes",
    ] {
        assert!(plain_safe(text, true), "{text}");
    }
    for text in [
        "",
        " lead",
        "trail ",
        "*.wikipedia.org",
        "a: b",
        "a #b",
        "1.20",
        "null",
        "yes",
        "- item",
        "end:",
        "line\nbreak",
        "---",
    ] {
        assert!(!plain_safe(text, false), "{text:?}");
    }
    assert!(plain_safe("a,b", false));
    assert!(!plain_safe("a,b", true));
}

#[test]
fn double_quoting_escapes_what_yaml_would_reinterpret() {
    assert_eq!(double_quoted("a\"b\\c"), "\"a\\\"b\\\\c\"");
    assert_eq!(double_quoted("x\ty\r\n\0"), "\"x\\ty\\r\\n\\0\"");
    assert_eq!(double_quoted("\u{7}\u{2028}"), "\"\\x07\\u2028\"");
}
