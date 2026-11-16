use super::*;

const PNG: [u8; 24] = [
    0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 0,
    1, 0, 0, 0, 1,
];

fn code(path: &str, bytes: &[u8]) -> Option<DiagnosticCode> {
    check_icon(path, bytes).err().map(|(code, _)| code)
}

#[test]
fn icons_are_judged_by_content_not_extension() {
    assert_eq!(check_icon("a.png", &PNG).ok(), Some(IconKind::Png));
    assert_eq!(code("a.png", b"<svg/>"), Some(DiagnosticCode::IconNotImage));
    assert_eq!(code("a.svg", &PNG), Some(DiagnosticCode::IconNotImage));
    assert_eq!(code("a.gif", b"GIF89a"), Some(DiagnosticCode::IconNotImage));
    assert_eq!(code("a.svg", b"hello"), Some(DiagnosticCode::IconNotImage));
    assert_eq!(
        code("a.svg", b"<html></html>"),
        Some(DiagnosticCode::IconNotImage)
    );
}

#[test]
fn svg_icons_carry_no_script_and_no_external_reference() {
    let clean = r##"<?xml version="1.0"?><!-- icon --><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><defs><linearGradient id="g"/></defs><circle cx="10" cy="10" r="6" fill="url(#g)"/><use href="#g"/></svg>"##;
    assert_eq!(
        check_icon("i.svg", clean.as_bytes()).ok(),
        Some(IconKind::Svg)
    );
    for (svg, expected) in [
        (
            r#"<svg><script>alert(1)</script></svg>"#,
            DiagnosticCode::SvgScript,
        ),
        (r#"<svg><foreignObject/></svg>"#, DiagnosticCode::SvgScript),
        (r#"<svg onload="x()"></svg>"#, DiagnosticCode::SvgScript),
        (
            r#"<svg><image href="https://e.com/a.png"/></svg>"#,
            DiagnosticCode::SvgExternalReference,
        ),
        (
            r#"<svg><use xlink:href="other.svg#a"/></svg>"#,
            DiagnosticCode::SvgExternalReference,
        ),
        (
            r#"<svg><rect style="fill:url(https://e.com/p)"/></svg>"#,
            DiagnosticCode::SvgExternalReference,
        ),
        (
            r#"<svg><style>@import url(x.css);</style></svg>"#,
            DiagnosticCode::SvgExternalReference,
        ),
        (
            r#"<!DOCTYPE svg [<!ENTITY x SYSTEM "file:///etc/passwd">]><svg/>"#,
            DiagnosticCode::SvgExternalReference,
        ),
    ] {
        assert_eq!(code("i.svg", svg.as_bytes()), Some(expected), "{svg}");
    }
}
