use super::extract::Tag;

#[derive(Clone, Debug)]
pub struct ScoredTag {
    pub file: String,
    pub tag: Tag,
    pub score: i64,
}

pub fn score_query(name: &str, query: &str) -> i64 {
    if query.trim().is_empty() {
        return 0;
    }
    let lower = name.to_lowercase();
    let mut s = 0i64;
    for tok in query.split_whitespace() {
        let t = tok.to_lowercase();
        if t.is_empty() {
            continue;
        }
        let mut start = 0usize;
        while let Some(pos) = lower[start..].find(&t) {
            s += 1;
            start += pos + t.len();
            if start >= lower.len() {
                break;
            }
        }
    }
    s
}

pub fn score_no_query(name: &str) -> i64 {
    name.chars().count() as i64
}

pub fn rank(scored: &mut [ScoredTag]) {
    scored.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.file.cmp(&b.file))
            .then_with(|| a.tag.line.cmp(&b.tag.line))
    });
}

#[allow(dead_code)]
pub fn estimate_tokens(bytes: usize) -> usize {
    bytes / 4
}

pub fn truncate_to_budget(scored: &[ScoredTag], max_tokens: usize) -> Vec<ScoredTag> {
    let mut out = Vec::new();
    let mut acc_bytes = 0usize;
    let max_bytes = max_tokens.saturating_mul(4);
    for s in scored {
        let cost = render_cost_bytes(s);
        if acc_bytes + cost > max_bytes && !out.is_empty() {
            break;
        }
        acc_bytes += cost;
        out.push(s.clone());
        if acc_bytes >= max_bytes {
            break;
        }
    }
    out
}

fn render_cost_bytes(s: &ScoredTag) -> usize {
    s.file.len() + 1 + 5 + 3 + s.tag.signature.len() + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(name: &str) -> Tag {
        Tag {
            name: name.into(),
            kind: "fn".into(),
            line: 1,
            signature: format!("fn {}()", name),
        }
    }

    fn st(file: &str, name: &str, score: i64) -> ScoredTag {
        ScoredTag {
            file: file.into(),
            tag: t(name),
            score,
        }
    }

    #[test]
    fn score_query_single_token_match() {
        assert_eq!(score_query("get_user", "user"), 1);
    }

    #[test]
    fn score_query_case_insensitive() {
        assert_eq!(score_query("GetUser", "user"), 1);
        assert_eq!(score_query("getuser", "USER"), 1);
    }

    #[test]
    fn score_query_multi_token_each_counts() {
        assert_eq!(score_query("auth_user_login", "auth login"), 2);
    }

    #[test]
    fn score_query_no_match_zero() {
        assert_eq!(score_query("foo_bar", "baz"), 0);
    }

    #[test]
    fn score_query_empty_zero() {
        assert_eq!(score_query("anything", ""), 0);
        assert_eq!(score_query("anything", "   "), 0);
    }

    #[test]
    fn score_query_repeated_substring() {
        assert_eq!(score_query("foo_foo_bar", "foo"), 2);
    }

    #[test]
    fn score_no_query_uses_length() {
        assert_eq!(score_no_query("abc"), 3);
        assert!(score_no_query("longer_name") > score_no_query("ab"));
    }

    #[test]
    fn rank_sorts_high_score_first() {
        let mut v = vec![st("a", "x", 1), st("b", "y", 5), st("c", "z", 3)];
        rank(&mut v);
        assert_eq!(v[0].tag.name, "y");
        assert_eq!(v[1].tag.name, "z");
        assert_eq!(v[2].tag.name, "x");
    }

    #[test]
    fn rank_stable_ties_by_file_then_line() {
        let a = st("z.rs", "a", 5);
        let mut b = st("a.rs", "b", 5);
        b.tag.line = 1;
        let mut c = st("a.rs", "c", 5);
        c.tag.line = 10;
        let mut v = vec![a.clone(), b.clone(), c.clone()];
        rank(&mut v);
        assert_eq!(v[0].file, "a.rs");
        assert_eq!(v[0].tag.line, 1);
        assert_eq!(v[1].file, "a.rs");
        assert_eq!(v[1].tag.line, 10);
        assert_eq!(v[2].file, "z.rs");
    }

    #[test]
    fn estimate_tokens_quarters_bytes() {
        assert_eq!(estimate_tokens(0), 0);
        assert_eq!(estimate_tokens(4), 1);
        assert_eq!(estimate_tokens(40), 10);
    }

    #[test]
    fn truncate_keeps_at_least_one_when_over_budget() {
        let v = vec![st(
            "really/long/file/path/that/exceeds.rs",
            "very_long_function_name_indeed",
            1,
        )];
        let out = truncate_to_budget(&v, 1);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn truncate_respects_budget() {
        let v: Vec<ScoredTag> = (0..50)
            .map(|i| st("a.rs", &format!("fn_{}", i), 1))
            .collect();
        let out = truncate_to_budget(&v, 50);
        assert!(out.len() < v.len());
        let bytes: usize = out.iter().map(render_cost_bytes).sum();
        assert!(estimate_tokens(bytes) <= 50 + 10);
    }

    #[test]
    fn truncate_empty_input_empty_output() {
        let out = truncate_to_budget(&[], 1000);
        assert!(out.is_empty());
    }

    #[test]
    fn truncate_zero_budget_returns_at_least_one() {
        let v = vec![st("a.rs", "x", 1)];
        let out = truncate_to_budget(&v, 0);
        assert_eq!(out.len(), 1);
    }
}
