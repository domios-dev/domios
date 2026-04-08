//! Synthetic preference extraction at write time.
//!
//! Ported from MemPalace's general_extractor.py + hybrid_v3 patterns:
//! free-form note/conversation/daily text is scanned for the 16 preference
//! patterns the LongMemEval team identified as covering most "User has
//! mentioned X" failure cases. Each match becomes a child preference
//! mem_item linked back via the `extracted_from` relation.
//!
//! Without this, FTS5 misses preference questions like "what database does
//! the user prefer?" because the source text says "I find Postgres more
//! reliable" — semantically distant from the question. The synthetic row
//! "User has mentioned: prefers Postgres" closes the gap.

use regex::Regex;
use std::sync::OnceLock;

struct PrefPattern {
    re: Regex,
    label: &'static str,
}

fn patterns() -> &'static Vec<PrefPattern> {
    static PATTERNS: OnceLock<Vec<PrefPattern>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        let raws: &[(&str, &str)] = &[
            (r"(?i)\bI usually (?:prefer |use |go with |pick |choose )(.{3,80}?)(?:\.|,|$|\bbut\b|\bbecause\b)", "usually"),
            (r"(?i)\bI always (?:use |prefer |pick |go with |do )(.{3,80}?)(?:\.|,|$|\bbut\b|\bbecause\b)", "always"),
            (r"(?i)\bI (?:really )?prefer (.{3,80}?)(?:\.|,|$|\bbut\b|\bbecause\b)", "prefer"),
            (r"(?i)\bI (?:don't|do not|never) (?:like|use|want) (.{3,80}?)(?:\.|,|$|\bbecause\b)", "dislike"),
            (r"(?i)\bI hate (.{3,80}?)(?:\.|,|$|\bbecause\b)", "hate"),
            (r"(?i)\bI love (.{3,80}?)(?:\.|,|$|\bbecause\b)", "love"),
            (r"(?i)\bI find (.{3,80}?) (?:more|much|the most) (?:reliable|useful|comfortable|productive)", "finds_X_more"),
            (r"(?i)\bMy favou?rite (.{3,80}?) is ([^.,]{3,80})", "favourite_is"),
            (r"(?i)\bI (?:tend to|typically) (.{3,80}?)(?:\.|,|$|\bbecause\b)", "tends_to"),
            (r"(?i)\bI'm (?:more|usually) (?:a |comfortable with )(.{3,80}?)(?:\.|,|$)", "self_describes_as"),
            (r"(?i)\bI work (?:mostly|primarily|always) (?:in|with|on) (.{3,80}?)(?:\.|,|$)", "works_with"),
            (r"(?i)\bI still remember (.{3,80}?)(?:\.|,|$)", "remembers"),
            (r"(?i)\bI used to (.{3,80}?)(?:\.|,|$)", "used_to"),
            (r"(?i)\bWhen I was (?:in (?:high school|college|university)|young|a kid|growing up)[,\s]+(.{3,80}?)(?:\.|,|$)", "when_young"),
            (r"(?i)\bI grew up (.{3,80}?)(?:\.|,|$)", "grew_up"),
            (r"(?i)\bMy (?:job|role|position) is (.{3,80}?)(?:\.|,|$)", "role_is"),
        ];
        raws.iter()
            .filter_map(|(p, l)| Regex::new(p).ok().map(|re| PrefPattern { re, label: l }))
            .collect()
    })
}

/// Returns one synthetic body string per match in `text`.
/// Empty result = nothing extractable.
pub fn extract_preferences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for pat in patterns() {
        for cap in pat.re.captures_iter(text) {
            let captured = cap
                .iter()
                .skip(1)
                .filter_map(|c| c.map(|m| m.as_str().trim()))
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" / ");
            if captured.is_empty() {
                continue;
            }
            out.push(format!("User has mentioned ({}): {}", pat.label, captured));
        }
    }
    // Dedupe
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_prefer_pattern() {
        let r = extract_preferences("I really prefer PostgreSQL because it's reliable");
        assert!(!r.is_empty(), "expected match for prefer");
        assert!(r.iter().any(|s| s.contains("PostgreSQL")));
    }

    #[test]
    fn extracts_multiple_in_one_text() {
        let r = extract_preferences(
            "I usually use vim for editing. I prefer Linux on servers. I hate JavaScript."
        );
        assert!(r.len() >= 2, "expected ≥2 matches, got {:?}", r);
    }

    #[test]
    fn extracts_nothing_from_neutral_text() {
        let r = extract_preferences("The build script ran for 30 seconds and produced 42 warnings.");
        assert!(r.is_empty(), "expected no extraction, got {:?}", r);
    }

    #[test]
    fn dedupes_identical_matches() {
        let r = extract_preferences("I prefer Rust. I prefer Rust.");
        assert_eq!(r.len(), 1);
    }
}
