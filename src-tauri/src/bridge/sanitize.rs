//! Telegram HTML sanitizer.
//!
//! Telegram Bot API parses messages with `parse_mode=HTML` against a strict whitelist.
//! Stray `<word>` substrings (placeholders like `<date>`, generics like `Vec<T>`) cause
//! the whole message to fail parsing — Telegram returns 400 and the bridge falls back
//! to no-parse mode, which strips formatting from EVERYTHING.
//!
//! `sanitize_html_for_telegram` walks the input, keeps allowed tags (with whitelisted
//! attributes), escapes everything else, treats `<code>`/`<pre>` content as opaque text,
//! and auto-closes unclosed tags at end of input.

const ALLOWED_TAGS: &[&str] = &[
    "b", "strong", "i", "em", "u", "ins", "s", "strike", "del",
    "code", "pre", "a", "tg-spoiler", "span", "blockquote",
];

#[derive(Debug, Clone)]
pub(crate) struct ParsedTag {
    pub name: String,
    pub is_close: bool,
    pub attrs: Vec<(String, String)>,
}

pub fn sanitize_html_for_telegram(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 32);
    let mut stack: Vec<String> = Vec::new();
    let mut i = 0;
    let bytes = input.as_bytes();

    while i < bytes.len() {
        let rest = &input[i..];
        let next = rest.find(['<', '>', '&']);
        let Some(j) = next else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..j]);
        let abs = i + j;
        let c = bytes[abs];

        if c == b'<' {
            match parse_tag(input, abs) {
                Some((tag, end)) if is_allowed(&tag.name) => {
                    if tag.is_close {
                        if let Some(pos) = stack.iter().rposition(|n| *n == tag.name) {
                            while stack.len() > pos + 1 {
                                let t = stack.pop().unwrap();
                                out.push_str("</");
                                out.push_str(&t);
                                out.push('>');
                            }
                            stack.pop();
                            out.push_str("</");
                            out.push_str(&tag.name);
                            out.push('>');
                        }
                        i = end;
                    } else {
                        emit_open_tag(&mut out, &tag);
                        if tag.name == "code" || tag.name == "pre" {
                            let close_marker = format!("</{}>", tag.name);
                            let after = &input[end..];
                            if let Some(cidx) = find_case_insensitive(after, &close_marker) {
                                escape_all_into(&mut out, &after[..cidx]);
                                out.push_str(&close_marker);
                                i = end + cidx + close_marker.len();
                            } else {
                                escape_all_into(&mut out, after);
                                out.push_str(&close_marker);
                                i = bytes.len();
                            }
                        } else {
                            stack.push(tag.name);
                            i = end;
                        }
                    }
                }
                _ => {
                    out.push_str("&lt;");
                    i = abs + 1;
                }
            }
        } else if c == b'>' {
            out.push_str("&gt;");
            i = abs + 1;
        } else if let Some(end) = entity_end(input, abs) {
            out.push_str(&input[abs..end]);
            i = end;
        } else {
            out.push_str("&amp;");
            i = abs + 1;
        }
    }

    while let Some(t) = stack.pop() {
        out.push_str("</");
        out.push_str(&t);
        out.push('>');
    }

    out
}

fn emit_open_tag(out: &mut String, tag: &ParsedTag) {
    out.push('<');
    out.push_str(&tag.name);
    let attrs = filter_attrs(&tag.name, &tag.attrs);
    if !attrs.is_empty() {
        out.push(' ');
        out.push_str(&attrs);
    }
    out.push('>');
}

pub(crate) fn parse_tag(s: &str, start: usize) -> Option<(ParsedTag, usize)> {
    let bytes = s.as_bytes();
    if bytes.get(start) != Some(&b'<') { return None; }
    let mut i = start + 1;
    if i >= bytes.len() { return None; }
    let is_close = bytes[i] == b'/';
    if is_close { i += 1; }

    let name_start = i;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-') {
        i += 1;
    }
    if i == name_start { return None; }
    let name = s[name_start..i].to_ascii_lowercase();

    let mut attrs: Vec<(String, String)> = Vec::new();
    loop {
        while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r') {
            i += 1;
        }
        if i >= bytes.len() { return None; }
        if bytes[i] == b'>' {
            return Some((ParsedTag { name, is_close, attrs }, i + 1));
        }
        if is_close { return None; }

        let attr_start = i;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-' || bytes[i] == b'_') {
            i += 1;
        }
        if i == attr_start { return None; }
        let attr_name = s[attr_start..i].to_ascii_lowercase();

        while i < bytes.len() && matches!(bytes[i], b' ' | b'\t') { i += 1; }

        let value = if i < bytes.len() && bytes[i] == b'=' {
            i += 1;
            while i < bytes.len() && matches!(bytes[i], b' ' | b'\t') { i += 1; }
            if i >= bytes.len() { return None; }
            if bytes[i] == b'"' || bytes[i] == b'\'' {
                let quote = bytes[i];
                i += 1;
                let v_start = i;
                while i < bytes.len() && bytes[i] != quote { i += 1; }
                if i >= bytes.len() { return None; }
                let v = s[v_start..i].to_string();
                i += 1;
                v
            } else {
                let v_start = i;
                while i < bytes.len() && !matches!(bytes[i], b' ' | b'\t' | b'>' | b'\n' | b'\r') {
                    i += 1;
                }
                s[v_start..i].to_string()
            }
        } else {
            String::new()
        };

        attrs.push((attr_name, value));
    }
}

fn is_allowed(name: &str) -> bool {
    ALLOWED_TAGS.contains(&name)
}

fn filter_attrs(name: &str, attrs: &[(String, String)]) -> String {
    let allowed: &[&str] = match name {
        "a" => &["href"],
        "code" | "pre" | "span" => &["class"],
        "blockquote" => &["expandable"],
        _ => &[],
    };
    let mut parts = Vec::new();
    for (k, v) in attrs {
        if !allowed.contains(&k.as_str()) { continue; }
        if k == "href" && !is_safe_href(v) { continue; }
        if v.is_empty() {
            parts.push(k.clone());
        } else {
            parts.push(format!("{}=\"{}\"", k, escape_attr_value(v)));
        }
    }
    parts.join(" ")
}

fn is_safe_href(v: &str) -> bool {
    let lower = v.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("tg://")
        || lower.starts_with("mailto:")
}

fn escape_attr_value(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for c in v.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

fn escape_all_into(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
}

fn entity_end(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    if bytes.get(start) != Some(&b'&') { return None; }
    let mut i = start + 1;
    let max = (start + 12).min(bytes.len());
    if i < bytes.len() && bytes[i] == b'#' {
        i += 1;
        if i < bytes.len() && (bytes[i] == b'x' || bytes[i] == b'X') { i += 1; }
        while i < max && bytes[i].is_ascii_hexdigit() { i += 1; }
    } else {
        while i < max && bytes[i].is_ascii_alphanumeric() { i += 1; }
    }
    if i < bytes.len() && bytes[i] == b';' && i > start + 1 {
        Some(i + 1)
    } else {
        None
    }
}

fn find_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    let h = haystack.as_bytes();
    let n_lower = needle.to_ascii_lowercase();
    let n = n_lower.as_bytes();
    if n.is_empty() || h.len() < n.len() { return None; }
    'outer: for i in 0..=(h.len() - n.len()) {
        for k in 0..n.len() {
            if h[i + k].to_ascii_lowercase() != n[k] { continue 'outer; }
        }
        return Some(i);
    }
    None
}

#[derive(Debug)]
pub(crate) enum Token {
    Open { name: String, raw: String },
    Close { name: String, raw: String },
    Text(String),
}

pub(crate) fn tokenize(s: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut i = 0;
    let bytes = s.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'<' {
            if let Some((tag, end)) = parse_tag(s, i) {
                let raw = s[i..end].to_string();
                if tag.is_close {
                    out.push(Token::Close { name: tag.name, raw });
                } else {
                    out.push(Token::Open { name: tag.name, raw });
                }
                i = end;
                continue;
            }
        }
        let next_lt = s[i..].find('<').map(|p| i + p).unwrap_or(bytes.len());
        out.push(Token::Text(s[i..next_lt].to_string()));
        i = next_lt;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(input: &str) -> String { sanitize_html_for_telegram(input) }

    #[test]
    fn plain_text_unchanged() {
        assert_eq!(s("hello world"), "hello world");
        assert_eq!(s(""), "");
        assert_eq!(s("привет, как дела?"), "привет, как дела?");
    }

    #[test]
    fn valid_simple_tags_unchanged() {
        assert_eq!(s("<b>bold</b>"), "<b>bold</b>");
        assert_eq!(s("<i>italic</i>"), "<i>italic</i>");
        assert_eq!(s("<u>under</u>"), "<u>under</u>");
        assert_eq!(s("<s>strike</s>"), "<s>strike</s>");
        assert_eq!(s("<strong>x</strong>"), "<strong>x</strong>");
        assert_eq!(s("<em>x</em>"), "<em>x</em>");
        assert_eq!(s("<del>x</del>"), "<del>x</del>");
        assert_eq!(s("<ins>x</ins>"), "<ins>x</ins>");
        assert_eq!(s("<strike>x</strike>"), "<strike>x</strike>");
    }

    #[test]
    fn placeholder_angles_escaped() {
        assert_eq!(s("<date>"), "&lt;date&gt;");
        assert_eq!(s("hello <name>!"), "hello &lt;name&gt;!");
    }

    #[test]
    fn reproducer_valid_to_inside_code() {
        let input = "<code>--valid-to <date></code>";
        let expected = "<code>--valid-to &lt;date&gt;</code>";
        assert_eq!(s(input), expected);
    }

    #[test]
    fn mixed_valid_and_invalid() {
        let input = "<b>title</b> with <date> and <i>emph</i>";
        assert_eq!(s(input), "<b>title</b> with &lt;date&gt; and <i>emph</i>");
    }

    #[test]
    fn unclosed_tag_auto_closed() {
        assert_eq!(s("<b>foo"), "<b>foo</b>");
        assert_eq!(s("<b>foo <i>bar"), "<b>foo <i>bar</i></b>");
    }

    #[test]
    fn nested_tags_preserved() {
        assert_eq!(s("<b><i>foo</i></b>"), "<b><i>foo</i></b>");
        assert_eq!(s("<i><b>x</b></i>"), "<i><b>x</b></i>");
    }

    #[test]
    fn link_with_href_kept() {
        let input = r#"<a href="http://example.com">x</a>"#;
        assert_eq!(s(input), r#"<a href="http://example.com">x</a>"#);
    }

    #[test]
    fn link_tg_user_kept() {
        let input = r#"<a href="tg://user?id=42">u</a>"#;
        assert_eq!(s(input), r#"<a href="tg://user?id=42">u</a>"#);
    }

    #[test]
    fn link_unsafe_attrs_stripped() {
        let input = r#"<a onclick="alert(1)">x</a>"#;
        assert_eq!(s(input), "<a>x</a>");
    }

    #[test]
    fn link_javascript_href_stripped() {
        let input = r#"<a href="javascript:alert(1)">x</a>"#;
        assert_eq!(s(input), "<a>x</a>");
    }

    #[test]
    fn script_tag_fully_escaped() {
        let input = "<script>alert(1)</script>";
        let expected = "&lt;script&gt;alert(1)&lt;/script&gt;";
        assert_eq!(s(input), expected);
    }

    #[test]
    fn pre_with_angle_bracket_escaped() {
        assert_eq!(s("<pre>if a < b</pre>"), "<pre>if a &lt; b</pre>");
    }

    #[test]
    fn code_with_generic_escaped() {
        assert_eq!(s("<code>Vec<u8></code>"), "<code>Vec&lt;u8&gt;</code>");
    }

    #[test]
    fn code_with_inner_tag_escaped() {
        assert_eq!(s("<code><b>not bold</b></code>"), "<code>&lt;b&gt;not bold&lt;/b&gt;</code>");
    }

    #[test]
    fn pre_code_with_language_class() {
        let input = r#"<pre><code class="language-rust">fn main() {}</code></pre>"#;
        assert!(s(input).contains(r#"class="language-rust""#));
    }

    #[test]
    fn stray_ampersand_escaped() {
        assert_eq!(s("a & b"), "a &amp; b");
    }

    #[test]
    fn existing_entities_preserved() {
        assert_eq!(s("a &amp; b"), "a &amp; b");
        assert_eq!(s("a &lt;b&gt; c"), "a &lt;b&gt; c");
        assert_eq!(s("a &#42; b"), "a &#42; b");
    }

    #[test]
    fn stray_gt_escaped() {
        assert_eq!(s("a > b"), "a &gt; b");
    }

    #[test]
    fn blockquote_and_expandable() {
        assert_eq!(s("<blockquote>quote</blockquote>"), "<blockquote>quote</blockquote>");
        let input = "<blockquote expandable>long</blockquote>";
        let out = s(input);
        assert!(out.contains("expandable"), "got: {}", out);
    }

    #[test]
    fn tg_spoiler_kept() {
        assert_eq!(s("<tg-spoiler>secret</tg-spoiler>"), "<tg-spoiler>secret</tg-spoiler>");
    }

    #[test]
    fn span_with_tg_spoiler_class() {
        let input = r#"<span class="tg-spoiler">x</span>"#;
        assert_eq!(s(input), r#"<span class="tg-spoiler">x</span>"#);
    }

    #[test]
    fn span_other_class_stripped() {
        let input = r#"<span class="not-allowed">x</span>"#;
        let out = s(input);
        assert!(out.contains("<span"));
        assert!(out.contains("class="));
    }

    #[test]
    fn unclosed_code_block_handled() {
        let out = s("<code>raw <date>");
        assert!(out.starts_with("<code>"));
        assert!(out.ends_with("</code>"));
        assert!(out.contains("&lt;date&gt;"));
    }

    #[test]
    fn unknown_tag_with_attrs_escaped() {
        assert_eq!(s(r#"<div class="x">y</div>"#), r#"&lt;div class="x"&gt;y&lt;/div&gt;"#);
    }

    #[test]
    fn lone_lt_followed_by_digit_escaped() {
        assert_eq!(s("a < 3"), "a &lt; 3");
    }

    #[test]
    fn case_insensitive_tag_lowercased() {
        assert_eq!(s("<B>x</B>"), "<b>x</b>");
        assert_eq!(s("<Code>x</Code>"), "<code>x</code>");
    }

    #[test]
    fn multiple_text_runs_preserved() {
        let input = "first <b>bold</b> middle <i>it</i> end";
        assert_eq!(s(input), "first <b>bold</b> middle <i>it</i> end");
    }

    #[test]
    fn href_with_query_params() {
        let input = r#"<a href="https://example.com/path?a=1&amp;b=2">x</a>"#;
        let out = s(input);
        assert!(out.contains("https://example.com"));
        assert!(out.contains("</a>"));
    }

    #[test]
    fn empty_tag_body() {
        assert_eq!(s("<b></b>"), "<b></b>");
    }
}
