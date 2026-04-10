//! Synthetic extraction at write time — 5 categories ported from MemPalace.
//!
//! Free-form text (note/conversation/daily) is scanned for regex patterns
//! across 5 memory types: Preference, Decision, Milestone, Problem, Emotional.
//! Each match becomes a child mem_item linked via `extracted_from` relation.
//!
//! This is the fast, synchronous, regex-based pass that runs inline during
//! save(). The heavier LLM-based extraction (entity detection, relation
//! creation) runs asynchronously via auto_extract graph workers.

use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractKind {
    Preference,
    Decision,
    Milestone,
    Problem,
    Emotional,
}

impl ExtractKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Preference => "preference",
            Self::Decision => "decision",
            Self::Milestone => "milestone",
            Self::Problem => "problem",
            Self::Emotional => "emotional",
        }
    }

    pub fn tag(&self) -> &'static str {
        match self {
            Self::Preference => "auto-extracted",
            Self::Decision => "auto-extracted,decision",
            Self::Milestone => "auto-extracted,milestone",
            Self::Problem => "auto-extracted,problem",
            Self::Emotional => "auto-extracted,emotional",
        }
    }
}

#[derive(Debug)]
pub struct Extraction {
    pub kind: ExtractKind,
    pub label: &'static str,
    pub captured: String,
}

struct Pattern {
    re: Regex,
    label: &'static str,
    kind: ExtractKind,
}

fn patterns() -> &'static Vec<Pattern> {
    static PATTERNS: OnceLock<Vec<Pattern>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        use ExtractKind::*;

        let raws: &[(&str, &str, ExtractKind)] = &[
            // ── Preference (original 16 + expanded) ──────────────────────
            (r"(?i)\bI usually (?:prefer |use |go with |pick |choose )(.{3,80}?)(?:\.|,|$|\bbut\b|\bbecause\b)", "usually", Preference),
            (r"(?i)\bI always (?:use |prefer |pick |go with |do )(.{3,80}?)(?:\.|,|$|\bbut\b|\bbecause\b)", "always", Preference),
            (r"(?i)\bI (?:really )?prefer (.{3,80}?)(?:\.|,|$|\bbut\b|\bbecause\b)", "prefer", Preference),
            (r"(?i)\bI (?:don't|do not|never) (?:like|use|want) (.{3,80}?)(?:\.|,|$|\bbecause\b)", "dislike", Preference),
            (r"(?i)\bI hate (.{3,80}?)(?:\.|,|$|\bbecause\b)", "hate", Preference),
            (r"(?i)\bI love (.{3,80}?)(?:\.|,|$|\bbecause\b)", "love", Preference),
            (r"(?i)\bI find (.{3,80}?) (?:more|much|the most) (?:reliable|useful|comfortable|productive)", "finds_X_more", Preference),
            (r"(?i)\bMy favou?rite (.{3,80}?) is ([^.,]{3,80})", "favourite_is", Preference),
            (r"(?i)\bI (?:tend to|typically) (.{3,80}?)(?:\.|,|$|\bbecause\b)", "tends_to", Preference),
            (r"(?i)\bI'm (?:more|usually) (?:a |comfortable with )(.{3,80}?)(?:\.|,|$)", "self_describes_as", Preference),
            (r"(?i)\bI work (?:mostly|primarily|always) (?:in|with|on) (.{3,80}?)(?:\.|,|$)", "works_with", Preference),
            (r"(?i)\bI still remember (.{3,80}?)(?:\.|,|$)", "remembers", Preference),
            (r"(?i)\bI used to (.{3,80}?)(?:\.|,|$)", "used_to", Preference),
            (r"(?i)\bWhen I was (?:in (?:high school|college|university)|young|a kid|growing up)[,\s]+(.{3,80}?)(?:\.|,|$)", "when_young", Preference),
            (r"(?i)\bI grew up (.{3,80}?)(?:\.|,|$)", "grew_up", Preference),
            (r"(?i)\bMy (?:job|role|position) is (.{3,80}?)(?:\.|,|$)", "role_is", Preference),
            // Expanded preference patterns from MemPalace
            (r"(?i)\bplease (?:always|never|don't) (.{3,80}?)(?:\.|,|$)", "please_rule", Preference),
            (r"(?i)\bmy (?:rule|preference|style|convention) is (.{3,80}?)(?:\.|,|$)", "my_rule_is", Preference),
            (r"(?i)\bwe (?:always|never) (.{3,80}?)(?:\.|,|$)", "we_always_never", Preference),
            (r"(?i)\buse\b(.{3,60}?)\binstead of\b(.{3,60}?)(?:\.|,|$)", "use_instead_of", Preference),

            // ── Decision ─────────────────────────────────────────────────
            (r"(?i)\blet'?s (?:use|go with|try|pick|choose|switch to) (.{3,80}?)(?:\.|,|$|\bbecause\b)", "lets_use", Decision),
            (r"(?i)\bwe (?:decided|chose|went with|picked|settled on) (.{3,80}?)(?:\.|,|$|\bbecause\b)", "we_decided", Decision),
            (r"(?i)\bi'?m going (?:to|with) (.{3,80}?)(?:\.|,|$|\bbecause\b)", "going_with", Decision),
            (r"(?i)\bbetter (?:to|than|approach|option) (.{3,80}?)(?:\.|,|$|\bbecause\b)", "better_to", Decision),
            (r"(?i)\binstead of (.{3,80}?)(?:\.|,|$)", "instead_of", Decision),
            (r"(?i)\brather than (.{3,80}?)(?:\.|,|$)", "rather_than", Decision),
            (r"(?i)\bthe reason (?:is|was|being) (.{3,80}?)(?:\.|,|$)", "reason_is", Decision),
            (r"(?i)\btrade-?off (.{3,80}?)(?:\.|,|$)", "tradeoff", Decision),
            (r"(?i)\bpros and cons (.{3,80}?)(?:\.|,|$)", "pros_and_cons", Decision),
            (r"(?i)\bwe should (.{3,80}?)(?:\.|,|$|\bbecause\b)", "we_should", Decision),
            (r"(?i)\bset (?:it |this )?to (.{3,80}?)(?:\.|,|$)", "set_to", Decision),
            (r"(?i)\bconfigure (.{3,80}?)(?:\.|,|$)", "configure", Decision),

            // ── Milestone ────────────────────────────────────────────────
            (r"(?i)\bgot it working(.{0,80}?)(?:\.|!|$)", "got_working", Milestone),
            (r"(?i)\bfigured (?:it )?out(.{0,80}?)(?:\.|!|$)", "figured_out", Milestone),
            (r"(?i)\bnailed it(.{0,80}?)(?:\.|!|$)", "nailed_it", Milestone),
            (r"(?i)\bcracked (?:it|the)(.{0,80}?)(?:\.|!|$)", "cracked_it", Milestone),
            (r"(?i)\bfinally (.{3,80}?)(?:\.|!|$)", "finally", Milestone),
            (r"(?i)\bfirst time (.{3,80}?)(?:\.|!|$)", "first_time", Milestone),
            (r"(?i)\bbreakthrough(.{0,80}?)(?:\.|!|$)", "breakthrough", Milestone),
            (r"(?i)\bshipped (.{3,80}?)(?:\.|!|$)", "shipped", Milestone),
            (r"(?i)\blaunched (.{3,80}?)(?:\.|!|$)", "launched", Milestone),
            (r"(?i)\bdeployed (.{3,80}?)(?:\.|!|$)", "deployed", Milestone),
            (r"(?i)\breleased (.{3,80}?)(?:\.|!|$)", "released", Milestone),
            (r"(?i)\bthe (?:key|trick) (?:is|was) (.{3,80}?)(?:\.|,|$)", "key_insight", Milestone),
            (r"(?i)\bnow I (?:understand|see|get it)(.{0,80}?)(?:\.|!|$)", "now_understand", Milestone),
            (r"(?i)\bturns out (.{3,80}?)(?:\.|,|$)", "turns_out", Milestone),
            (r"(?i)(\d+x (?:compression|faster|slower|better|improvement|reduction))", "metric_Nx", Milestone),
            (r"(?i)(\d+% (?:reduction|improvement|faster|better|smaller))", "metric_pct", Milestone),

            // ── Problem ──────────────────────────────────────────────────
            (r"(?i)\b(?:bug|error|crash) (?:in|with|when) (.{3,80}?)(?:\.|,|$)", "bug_in", Problem),
            (r"(?i)\bdoesn'?t work(.{0,80}?)(?:\.|,|$)", "doesnt_work", Problem),
            (r"(?i)\bnot working(.{0,80}?)(?:\.|,|$)", "not_working", Problem),
            (r"(?i)\bkeeps? (?:failing|crashing|breaking|erroring)(.{0,80}?)(?:\.|,|$)", "keeps_failing", Problem),
            (r"(?i)\broot cause (.{3,80}?)(?:\.|,|$)", "root_cause", Problem),
            (r"(?i)\bthe (?:problem|issue|bug) (?:is|was) (.{3,80}?)(?:\.|,|$)", "problem_is", Problem),
            (r"(?i)\bthe fix (?:is|was) (.{3,80}?)(?:\.|,|$)", "fix_is", Problem),
            (r"(?i)\bworkaround (.{3,80}?)(?:\.|,|$)", "workaround", Problem),
            (r"(?i)\bthat'?s why (.{3,80}?)(?:\.|,|$)", "thats_why", Problem),
            (r"(?i)\bsolution (?:is|was) (.{3,80}?)(?:\.|,|$)", "solution_is", Problem),
            (r"(?i)\bresolved (.{3,80}?)(?:\.|,|$)", "resolved", Problem),
            (r"(?i)\bpatched (.{3,80}?)(?:\.|,|$)", "patched", Problem),
            (r"(?i)\bfixed (?:it |the |by )(.{3,80}?)(?:\.|,|$)", "fixed_by", Problem),

            // ── Emotional ────────────────────────────────────────────────
            (r"(?i)\bI feel (.{3,80}?)(?:\.|,|$)", "i_feel", Emotional),
            (r"(?i)\bI'm (?:scared|afraid|worried|anxious) (.{0,80}?)(?:\.|,|$)", "scared", Emotional),
            (r"(?i)\bI'm (?:proud|happy|thrilled|excited) (.{0,80}?)(?:\.|,|$)", "proud", Emotional),
            (r"(?i)\bI (?:can't|cannot) (.{3,80}?)(?:\.|,|$)", "cant", Emotional),
            (r"(?i)\bI wish (.{3,80}?)(?:\.|,|$)", "wish", Emotional),
            (r"(?i)\bI miss (.{3,80}?)(?:\.|,|$)", "miss", Emotional),
            (r"(?i)\bnever told anyone (.{3,80}?)(?:\.|,|$)", "never_told", Emotional),
            (r"(?i)\bnobody knows (.{3,80}?)(?:\.|,|$)", "nobody_knows", Emotional),
        ];

        raws.iter()
            .filter_map(|(p, l, k)| {
                Regex::new(p).ok().map(|re| Pattern { re, label: l, kind: *k })
            })
            .collect()
    })
}

/// Extract all matching patterns from text. Returns typed extractions.
pub fn extract_all(text: &str) -> Vec<Extraction> {
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
            // For milestone/emotional patterns the capture group may be empty
            // (e.g. "Got it working!" — the match itself is the content).
            // Fall back to the full match text.
            let text = if captured.is_empty() {
                cap.get(0).map(|m| m.as_str().trim().to_string()).unwrap_or_default()
            } else {
                captured
            };
            if text.is_empty() {
                continue;
            }
            out.push(Extraction {
                kind: pat.kind,
                label: pat.label,
                captured: text,
            });
        }
    }
    // Dedupe by (kind, captured)
    out.sort_by(|a, b| {
        a.kind.as_str().cmp(b.kind.as_str()).then(a.captured.cmp(&b.captured))
    });
    out.dedup_by(|a, b| a.kind == b.kind && a.captured == b.captured);
    out
}

/// Legacy compat: returns preference-only extractions as formatted strings.
pub fn extract_preferences(text: &str) -> Vec<String> {
    extract_all(text)
        .into_iter()
        .filter(|e| e.kind == ExtractKind::Preference)
        .map(|e| format!("User has mentioned ({}): {}", e.label, e.captured))
        .collect()
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

    // ── New 5-category tests ──

    #[test]
    fn extracts_decisions() {
        let r = extract_all("Let's use Redis for caching because it's fast.");
        assert!(r.iter().any(|e| e.kind == ExtractKind::Decision), "expected decision, got {:?}", r);
    }

    #[test]
    fn extracts_milestones() {
        let r = extract_all("Finally got the auth flow working!");
        assert!(r.iter().any(|e| e.kind == ExtractKind::Milestone), "expected milestone, got {:?}", r);
    }

    #[test]
    fn extracts_problems() {
        let r = extract_all("The problem was the JWT token expiring too early.");
        assert!(r.iter().any(|e| e.kind == ExtractKind::Problem), "expected problem, got {:?}", r);
    }

    #[test]
    fn extracts_emotional() {
        let r = extract_all("I feel like this project is going really well.");
        assert!(r.iter().any(|e| e.kind == ExtractKind::Emotional), "expected emotional, got {:?}", r);
    }

    #[test]
    fn mixed_text_extracts_multiple_kinds() {
        let text = "I prefer Postgres. We decided to use Docker. Got it working! The bug was in the auth middleware.";
        let r = extract_all(text);
        let kinds: Vec<&str> = r.iter().map(|e| e.kind.as_str()).collect();
        assert!(kinds.contains(&"preference"), "missing preference in {:?}", kinds);
        assert!(kinds.contains(&"decision"), "missing decision in {:?}", kinds);
        assert!(kinds.contains(&"milestone"), "missing milestone in {:?}", kinds);
    }

    // ── Comprehensive pattern coverage ──

    #[test]
    fn decision_we_should() {
        let r = extract_all("We should migrate to Kubernetes.");
        assert!(r.iter().any(|e| e.kind == ExtractKind::Decision && e.label == "we_should"));
    }

    #[test]
    fn decision_instead_of() {
        let r = extract_all("Instead of MySQL we went with Postgres.");
        assert!(r.iter().any(|e| e.kind == ExtractKind::Decision && e.label == "instead_of"));
    }

    #[test]
    fn milestone_shipped() {
        let r = extract_all("Shipped the new onboarding flow to production.");
        assert!(r.iter().any(|e| e.kind == ExtractKind::Milestone && e.label == "shipped"));
    }

    #[test]
    fn milestone_metric() {
        let r = extract_all("We achieved 3x compression improvement.");
        assert!(r.iter().any(|e| e.kind == ExtractKind::Milestone && e.label == "metric_Nx"), "got {:?}", r);
    }

    #[test]
    fn problem_root_cause() {
        let r = extract_all("Root cause was a race condition in the mutex.");
        assert!(r.iter().any(|e| e.kind == ExtractKind::Problem && e.label == "root_cause"));
    }

    #[test]
    fn problem_workaround() {
        let r = extract_all("Workaround is to restart the service every 4 hours.");
        assert!(r.iter().any(|e| e.kind == ExtractKind::Problem && e.label == "workaround"));
    }

    #[test]
    fn emotional_i_feel() {
        let r = extract_all("I feel overwhelmed by the amount of technical debt.");
        assert!(r.iter().any(|e| e.kind == ExtractKind::Emotional && e.label == "i_feel"));
    }

    #[test]
    fn preference_please_rule() {
        let r = extract_all("Please always run tests before pushing.");
        assert!(r.iter().any(|e| e.kind == ExtractKind::Preference && e.label == "please_rule"));
    }

    #[test]
    fn preference_use_instead_of() {
        let r = extract_all("Use FastAPI instead of Django for real-time endpoints.");
        assert!(r.iter().any(|e| e.kind == ExtractKind::Preference && e.label == "use_instead_of"));
    }

    // ── False positive resistance ──

    #[test]
    fn no_false_positive_on_code() {
        let r = extract_all("fn main() { let x = 42; println!(\"{}\", x); }");
        assert!(r.is_empty(), "code should not trigger extraction: {:?}", r);
    }

    #[test]
    fn no_false_positive_on_log() {
        let r = extract_all("[2026-04-09 15:30:00] INFO: Request completed in 234ms, status=200");
        assert!(r.is_empty(), "log line should not trigger: {:?}", r);
    }

    #[test]
    fn no_false_positive_on_json() {
        let r = extract_all(r#"{"name": "test", "value": 42, "status": "ok"}"#);
        assert!(r.is_empty(), "JSON should not trigger: {:?}", r);
    }

    #[test]
    fn no_false_positive_short_text() {
        let r = extract_all("ok done");
        assert!(r.is_empty(), "short text should not trigger: {:?}", r);
    }
}
