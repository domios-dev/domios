use std::collections::BTreeMap;

use super::budget::ScoredTag;

pub fn render(scored: &[ScoredTag]) -> String {
    let mut by_file: BTreeMap<String, Vec<&ScoredTag>> = BTreeMap::new();
    for s in scored {
        by_file.entry(s.file.clone()).or_default().push(s);
    }
    let mut out = String::new();
    for (file, tags) in by_file {
        out.push_str("## ");
        out.push_str(&file);
        out.push('\n');
        let mut sorted = tags.clone();
        sorted.sort_by_key(|s| s.tag.line);
        for s in sorted {
            out.push_str(&file);
            out.push(':');
            out.push_str(&s.tag.line.to_string());
            out.push_str(" | ");
            out.push_str(&s.tag.signature);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::extract::Tag;
    use super::*;

    fn st(file: &str, name: &str, line: u32, sig: &str, score: i64) -> ScoredTag {
        ScoredTag {
            file: file.into(),
            tag: Tag {
                name: name.into(),
                kind: "fn".into(),
                line,
                signature: sig.into(),
            },
            score,
        }
    }

    #[test]
    fn render_groups_by_file_with_heading() {
        let v = vec![st("a.rs", "x", 5, "fn x()", 1), st("a.rs", "y", 1, "fn y()", 2)];
        let r = render(&v);
        assert!(r.contains("## a.rs"));
        assert!(r.contains("a.rs:1 | fn y()"));
        assert!(r.contains("a.rs:5 | fn x()"));
    }

    #[test]
    fn render_groups_two_files() {
        let v = vec![
            st("a.rs", "x", 1, "fn x()", 1),
            st("b.rs", "y", 1, "fn y()", 1),
        ];
        let r = render(&v);
        assert!(r.contains("## a.rs"));
        assert!(r.contains("## b.rs"));
    }

    #[test]
    fn render_lines_within_file_sorted_ascending() {
        let v = vec![
            st("f.rs", "c", 30, "fn c()", 1),
            st("f.rs", "a", 5, "fn a()", 1),
            st("f.rs", "b", 20, "fn b()", 1),
        ];
        let r = render(&v);
        let pos_a = r.find("fn a()").unwrap();
        let pos_b = r.find("fn b()").unwrap();
        let pos_c = r.find("fn c()").unwrap();
        assert!(pos_a < pos_b);
        assert!(pos_b < pos_c);
    }

    #[test]
    fn render_empty_input_empty_output() {
        let r = render(&[]);
        assert!(r.is_empty());
    }

    #[test]
    fn render_format_path_line_pipe_signature() {
        let v = vec![st("path/to/x.rs", "do", 42, "pub fn do(x: i32)", 1)];
        let r = render(&v);
        assert!(r.contains("path/to/x.rs:42 | pub fn do(x: i32)"));
    }
}
