use codeterm_decision::{DecisionAnswer, DecisionAnswers};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_QUERY_CHARS: usize = 80;
pub const MAX_QUESTION_CHARS: usize = 160;
pub const MAX_ANSWER_CHARS: usize = 160;
pub const MAX_NAME_CHARS: usize = 64;
pub const MAX_QUESTIONS: usize = 64;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct QuestionAnswer {
    pub name: String,
    pub question: String,
    pub answer: String,
}

impl QuestionAnswer {
    pub fn new(name: &str, question: &str, answer: &str) -> Self {
        Self {
            name: scrub(name, MAX_NAME_CHARS),
            question: scrub(question, MAX_QUESTION_CHARS),
            answer: scrub(answer, MAX_ANSWER_CHARS),
        }
    }
}

pub fn scrub(text: &str, cap: usize) -> String {
    static TOKENS: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let tokens = TOKENS.get_or_init(|| {
        regex::Regex::new(r"\b(?:sk-|ghp_|github_pat_|xox[baprs]-)[A-Za-z0-9_-]+")
            .expect("credential prefix pattern is valid")
    });
    let redacted = codeterm_worker_services::launch_diagnostics::redact_secrets(text);
    let redacted = tokens.replace_all(&redacted, "<redacted>");
    let printable: String = redacted
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    let line = printable.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= cap {
        line
    } else {
        line.chars()
            .take(cap.saturating_sub(1))
            .chain(['…'])
            .collect()
    }
}

pub(super) fn project(value: &Value) -> Option<Vec<QuestionAnswer>> {
    let questions: Vec<_> = value
        .as_array()?
        .iter()
        .take(MAX_QUESTIONS)
        .filter_map(|pair| {
            Some(QuestionAnswer::new(
                pair["name"].as_str().unwrap_or(""),
                pair["question"].as_str()?,
                pair["answer"].as_str()?,
            ))
        })
        .collect();
    (!questions.is_empty()).then_some(questions)
}

pub enum ActivityContext {
    Rerank {
        query: String,
        titles: Vec<String>,
    },
    TabState {
        title: String,
        declared_safe_choice: Option<String>,
        decision_point: String,
    },
    Sentinel {
        job: String,
        buckets: Vec<String>,
    },
}

impl ActivityContext {
    pub fn decision_point(&self) -> &str {
        match self {
            Self::Rerank { .. } => "memory_rerank",
            Self::TabState { decision_point, .. } => decision_point,
            Self::Sentinel { .. } => "blueprint",
        }
    }

    pub fn failure(&self, reason: &str) -> Vec<QuestionAnswer> {
        let question = match self {
            Self::Rerank { query, .. } => {
                format!("Which memories answer: {}?", scrub(query, MAX_QUERY_CHARS))
            }
            Self::TabState { title, .. } => {
                format!("What state is tab {} in?", scrub(title, MAX_QUERY_CHARS))
            }
            Self::Sentinel { job, .. } => {
                format!(
                    "What state is Blueprint {} in?",
                    scrub(job, MAX_QUERY_CHARS)
                )
            }
        };
        let answer = match reason {
            "budget" | "budget_exhausted" => "Request cap reached",
            "credit_exhausted" => "Credit exhausted",
            "deadline" | "model_deadline" => "Decision deadline exceeded",
            "unchanged" => "State unchanged",
            "timeout" => "Watch deadline exceeded",
            "probe_capture_timeout" => "Probe capture deadline exceeded",
            "probe_capture_failed" => "Probe capture failed",
            "probe_containment_failed" => "Probe could not be contained",
            _ => "Decision unavailable",
        };
        vec![QuestionAnswer::new("decision", &question, answer)]
    }

    pub fn answers(&self, answers: &DecisionAnswers) -> Vec<QuestionAnswer> {
        answers
            .iter()
            .take(MAX_QUESTIONS)
            .map(|(name, answer)| {
                let (question, text) = match self {
                    Self::Rerank { query, titles } => {
                        let title = name
                            .strip_prefix("candidate_")
                            .and_then(|index| index.parse::<usize>().ok())
                            .and_then(|index| titles.get(index));
                        let subject = title.map_or_else(
                            || "this memory".into(),
                            |title| scrub(title, MAX_QUERY_CHARS),
                        );
                        (
                            format!("Does {subject} answer: {}?", scrub(query, MAX_QUERY_CHARS)),
                            probability(answer),
                        )
                    }
                    Self::TabState {
                        title,
                        declared_safe_choice,
                        ..
                    } => {
                        let title = scrub(title, MAX_QUERY_CHARS);
                        match name.as_str() {
                            "label" => (
                                format!("What state is tab {title} in?"),
                                choice(answer).map(bucket).unwrap_or("Unknown").into(),
                            ),
                            "prompt_visible" => (
                                format!("Is tab {title} waiting for input?"),
                                probability(answer),
                            ),
                            "safe_choice" => (
                                format!("Is a safe response available for tab {title}?"),
                                match choice(answer) {
                                    Some("__abstain__" | "other") | None => {
                                        "No safe response selected"
                                    }
                                    Some(selected)
                                        if Some(selected) == declared_safe_choice.as_deref() =>
                                    {
                                        "Provider-declared safe option selected"
                                    }
                                    Some(_) => "Another option selected",
                                }
                                .into(),
                            ),
                            _ => ("What was decided?".into(), "Unknown".into()),
                        }
                    }
                    Self::Sentinel { job, buckets } => (
                        format!(
                            "What state is Blueprint {} in?",
                            scrub(job, MAX_QUERY_CHARS)
                        ),
                        choice(answer)
                            .filter(|choice| {
                                buckets.iter().any(|bucket| bucket.as_str() == *choice)
                            })
                            .map(|choice| choice.replace('_', " "))
                            .unwrap_or_else(|| "Other".into()),
                    ),
                };
                QuestionAnswer::new(name, &question, &text)
            })
            .collect()
    }
}

pub fn sentinel(
    job: &str,
    buckets: &[String],
    label: Option<&str>,
    reason: &str,
) -> Vec<QuestionAnswer> {
    let context = ActivityContext::Sentinel {
        job: job.into(),
        buckets: buckets.to_vec(),
    };
    match label {
        Some(label) => context.answers(&DecisionAnswers::from([(
            "bucket".into(),
            DecisionAnswer::Choice {
                choice: label.into(),
                probabilities: Default::default(),
                confidence: None,
            },
        )])),
        None => context.failure(reason),
    }
}

fn choice(answer: &DecisionAnswer) -> Option<&str> {
    match answer {
        DecisionAnswer::Choice { choice, .. } => Some(choice),
        _ => None,
    }
}

fn probability(answer: &DecisionAnswer) -> String {
    match answer {
        DecisionAnswer::Noul { p } if p.is_finite() && (0.0..=1.0).contains(p) => {
            format!("{:.0}% likely", p * 100.0)
        }
        DecisionAnswer::Score { score, .. } if score.is_finite() => {
            format!("Relevance score {score:.2}")
        }
        _ => "Unknown".into(),
    }
}

pub fn bucket(label: &str) -> &'static str {
    match label {
        "ready_idle" => "Ready and idle",
        "working_normally" => "Working normally",
        "provider_update_prompt" => "Provider update awaiting a response",
        "trust_or_permission_prompt" => "Trust or permission awaiting a response",
        "login_or_auth_required" => "Login required",
        "model_unavailable_or_quota" => "Model unavailable or quota exhausted",
        "crashed_to_shell" => "Agent exited to the shell",
        "dead_pty" => "Terminal process stopped",
        "error_loop" => "Repeated errors",
        "still_starting" => "Still starting",
        _ => "Uncertain",
    }
}

pub fn tab_state(title: &str, label: &str) -> Vec<QuestionAnswer> {
    vec![QuestionAnswer::new(
        "label",
        &format!("What state is tab {} in?", scrub(title, MAX_QUERY_CHARS)),
        bucket(label),
    )]
}

#[cfg(test)]
pub fn rerank(query: &str, title: Option<&str>, count: usize) -> Vec<QuestionAnswer> {
    let answer = title.map_or_else(
        || "No memories ranked".into(),
        |title| {
            format!(
                "{} (+{} ranked)",
                scrub(title, MAX_QUERY_CHARS),
                count.saturating_sub(1)
            )
        },
    );
    vec![QuestionAnswer::new(
        "ranking",
        &format!("Which memories answer: {}?", scrub(query, MAX_QUERY_CHARS)),
        &answer,
    )]
}

pub fn recovery(title: &str, reason: &str) -> Vec<QuestionAnswer> {
    let answer = match reason {
        "provider_declared_safe_choice" => "Provider-declared safe response authorized",
        "deterministic_directory_trust" => "Folder trust response authorized",
        "resumed" => "Resumed the owned session",
        "fresh_launch" => "Started a fresh agent session",
        _ => "Restart authorized after confirmed process exit",
    };
    vec![QuestionAnswer::new(
        "recovery",
        &format!("How should tab {} recover?", scrub(title, MAX_QUERY_CHARS)),
        answer,
    )]
}

pub fn provider_update(label: &str) -> Vec<QuestionAnswer> {
    let answer = match label {
        "update_started" => "Started the declared update",
        "update_succeeded" => "Update succeeded",
        "update_failed" => "Update failed",
        "restart_requested" => "Requested an idle tab restart",
        "resumed" => "Resumed the owned session",
        "fresh_launch" => "Started a fresh agent session",
        "restart_failed" => "Restart failed",
        "safe_choice_answered" => "Answered the declared safe option",
        "interrupted" => "Update or restart was interrupted",
        _ => "Unknown",
    };
    vec![QuestionAnswer::new(
        "provider_update",
        "What happened to the provider update?",
        answer,
    )]
}

#[cfg(test)]
mod tests;
