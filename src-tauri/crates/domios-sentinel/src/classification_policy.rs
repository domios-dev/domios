use super::{
    machine::{regex_verdict_if_matched, Verdict},
    provenance::Classification,
};
use std::collections::BTreeMap;

#[derive(Debug, PartialEq)]
pub enum Policy {
    Pattern(Verdict),
    Unmatched,
    Model,
}

pub fn select(
    patterns: &BTreeMap<String, String>,
    output: &str,
    model_classification: bool,
) -> Policy {
    if !patterns.is_empty() {
        regex_verdict_if_matched(patterns, output)
            .map(Policy::Pattern)
            .unwrap_or(Policy::Unmatched)
    } else if model_classification {
        Policy::Model
    } else {
        Policy::Unmatched
    }
}

pub fn classify(
    patterns: &BTreeMap<String, String>,
    output: &str,
    model_classification: bool,
    model: impl FnOnce() -> Classification,
) -> Classification {
    match select(patterns, output, model_classification) {
        Policy::Pattern(verdict) => Classification::regex(verdict),
        Policy::Unmatched => Classification::regex(Verdict {
            bucket: "other".into(),
            confidence: 0.0,
        }),
        Policy::Model => model(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn declared_patterns_never_call_the_model_even_for_ten_changed_unmatched_ticks() {
        let patterns = BTreeMap::from([("ready".into(), "^ready$".into())]);
        for tick in 0..10 {
            for opt_in in [false, true] {
                let verdict = classify(&patterns, &format!("unmatched-{tick}"), opt_in, || {
                    panic!("patterns must stay deterministic")
                });
                assert_eq!(verdict.result.unwrap().bucket, "other");
            }
        }
        assert!(matches!(
            select(&patterns, "ready\r\n", false),
            Policy::Pattern(_)
        ));
    }

    #[test]
    fn only_explicit_model_opt_in_calls_the_model() {
        let patterns = BTreeMap::new();
        assert_eq!(select(&patterns, "unknown", false), Policy::Unmatched);
        let calls = Cell::new(0);
        let verdict = classify(&patterns, "unknown", true, || {
            calls.set(calls.get() + 1);
            Classification::adapter(Ok(Verdict {
                bucket: "ready".into(),
                confidence: 1.0,
            }))
        });
        assert_eq!(calls.get(), 1);
        assert_eq!(verdict.result.unwrap().bucket, "ready");
    }
}
