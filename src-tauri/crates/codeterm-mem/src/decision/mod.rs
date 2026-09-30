//! Provider-agnostic System-One decision capability.
//!
//! Implementations are resolved through the daemon's
//! `codeterm_plugin::capability::CapabilityRegistry<dyn DecisionModel>`; this
//! crate only defines the trait so it never depends on `codeterm-plugin`.

pub mod rerank;
pub mod token_store;
pub mod typesafe;

use std::collections::BTreeMap;
use std::fmt;
use std::time::Instant;

/// A decision adapter. Callers branch only on [`DecisionModel::primitives`], never on `id`.
pub trait DecisionModel: Send + Sync {
    fn id(&self) -> &str;
    fn primitives(&self) -> DecisionPrimitives;
    fn decide(
        &self,
        req: &DecisionRequest,
        deadline: Instant,
    ) -> Result<DecisionAnswer, DecisionError>;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DecisionPrimitives {
    pub choice: bool,
    pub noul: bool,
    pub score: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionQuestion {
    Choice {
        options: BTreeMap<String, String>,
    },
    Noul,
    /// `levels` are ordered low -> high.
    Score {
        levels: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionRequest {
    pub state: String,
    pub instructions: String,
    pub question: DecisionQuestion,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DecisionAnswer {
    Choice {
        choice: String,
        probabilities: BTreeMap<String, f64>,
        confidence: Option<f64>,
    },
    Noul {
        p: f64,
    },
    /// `probabilities` are keyed by level index as a string, `"0"..="L-1"`.
    Score {
        score: f64,
        probabilities: BTreeMap<String, f64>,
        confidence: Option<f64>,
    },
}

/// Adapters must build the `String` payloads from sanitized reasons: never a request body or token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionError {
    Unavailable(String),
    Unsupported,
    Deadline,
    Http(u16),
    Transport(String),
    Parse(String),
}

impl fmt::Display for DecisionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(reason) => write!(f, "decision model unavailable: {reason}"),
            Self::Unsupported => f.write_str("decision primitive unsupported"),
            Self::Deadline => f.write_str("decision deadline exceeded"),
            Self::Http(status) => write!(f, "decision http status {status}"),
            Self::Transport(reason) => write!(f, "decision transport error: {reason}"),
            Self::Parse(reason) => write!(f, "decision parse error: {reason}"),
        }
    }
}

impl std::error::Error for DecisionError {}

#[cfg(test)]
mod tests;
