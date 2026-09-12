//! The one honest way to build a [`ClassifyInput`].
//!
//! Both the live PTY path and the offline replay harness construct their
//! classifier input here, so the corpus measures the program the user runs.
//! `status_bar_rows` is a required input with no default: it is either resolved
//! from the registry (and an unresolvable provider is a typed error, never a
//! zero) or supplied as explicit fixture data by a harness that has no registry.

use super::offline_state_classifier::{ClassifyInput, StatusElapsedProgress};
use super::signal_extractor::{Motion, VolatileHint};

/// Capability lookup, never identity: the only question asked is how many
/// bottom rows THIS resolved provider repaints. `None` means the provider is
/// not resolvable, which is an error, not a zero.
pub trait StatusBarRowsSource {
    fn status_bar_rows_for(&self, agent_type: &str) -> Option<u16>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatusBarRowsError {
    UnresolvedProvider { agent_type: String },
}

impl std::fmt::Display for StatusBarRowsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnresolvedProvider { agent_type } => write!(
                f,
                "provider {agent_type:?} is not resolvable: status-bar geometry unknown"
            ),
        }
    }
}

impl std::error::Error for StatusBarRowsError {}

/// A status-bar row count that is KNOWN. There is deliberately no `Default`,
/// no `From<u16>` and no public tuple constructor: every value is either
/// resolved from a provider or declared as fixture data at its call site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusBarRows(u16);

impl StatusBarRows {
    /// A surface that genuinely has no provider chrome (a plain shell tab).
    /// NEVER a fallback for a provider that failed to resolve — that is an
    /// error, and `resolve` returns one.
    pub const NO_STATUS_BAR: Self = Self(0);

    /// Explicit geometry for a synthetic fixture or a harness with no registry.
    pub const fn fixture(rows: u16) -> Self {
        Self(rows)
    }

    pub fn resolve<S>(source: &S, agent_type: &str) -> Result<Self, StatusBarRowsError>
    where
        S: StatusBarRowsSource + ?Sized,
    {
        source
            .status_bar_rows_for(agent_type)
            .map(StatusBarRows)
            .ok_or_else(|| StatusBarRowsError::UnresolvedProvider {
                agent_type: agent_type.to_string(),
            })
    }

    pub const fn get(self) -> u16 {
        self.0
    }

    /// The semantic-clock volatile band. The offline extractor and the live
    /// content hash both exclude exactly these rows.
    pub const fn volatile_hint(self) -> VolatileHint {
        VolatileHint {
            bottom_rows: self.0,
        }
    }
}

/// The provider-neutral per-tick observation. Everything here is measured from
/// the stream; nothing about the provider's geometry lives in it.
pub struct ClassifyParts<'a> {
    pub picker_question: Option<&'a str>,
    pub screen_lines: &'a [String],
    pub tail: &'a str,
    pub byte_idle_ms: u64,
    pub semantic_idle_ms: u64,
    pub motion: Motion,
    pub cursor_hidden: bool,
    pub composer_ready: bool,
    pub status_elapsed: StatusElapsedProgress,
}

/// Holds the one thing that must never default. A builder cannot exist until
/// the rows are known, so a `ClassifyInput` cannot exist without them either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClassifyInputBuilder {
    status_bar_rows: StatusBarRows,
}

impl ClassifyInputBuilder {
    pub fn for_provider<S>(source: &S, agent_type: &str) -> Result<Self, StatusBarRowsError>
    where
        S: StatusBarRowsSource + ?Sized,
    {
        StatusBarRows::resolve(source, agent_type).map(Self::for_rows)
    }

    pub const fn for_rows(status_bar_rows: StatusBarRows) -> Self {
        Self { status_bar_rows }
    }

    pub const fn status_bar_rows(&self) -> StatusBarRows {
        self.status_bar_rows
    }

    pub const fn volatile_hint(&self) -> VolatileHint {
        self.status_bar_rows.volatile_hint()
    }

    /// The single place a `ClassifyInput` is born. Omitting the band is a
    /// compile error, not a zero:
    ///
    /// ```compile_fail
    /// use codeterm_lib::agent::offline_state_classifier::ClassifyInput;
    /// use codeterm_lib::agent::signal_extractor::Motion;
    /// let lines: Vec<String> = Vec::new();
    /// let _ = ClassifyInput {
    ///     picker_question: None,
    ///     screen_lines: &lines,
    ///     tail: "",
    ///     byte_idle_ms: 0,
    ///     semantic_idle_ms: 0,
    ///     motion: Motion::Still,
    ///     cursor_hidden: false,
    ///     composer_ready: false,
    ///     status_elapsed: Default::default(),
    /// };
    /// ```
    pub fn build<'a>(&self, parts: ClassifyParts<'a>) -> ClassifyInput<'a> {
        ClassifyInput {
            status_bar_rows: self.status_bar_rows,
            picker_question: parts.picker_question,
            screen_lines: parts.screen_lines,
            tail: parts.tail,
            byte_idle_ms: parts.byte_idle_ms,
            semantic_idle_ms: parts.semantic_idle_ms,
            motion: parts.motion,
            cursor_hidden: parts.cursor_hidden,
            composer_ready: parts.composer_ready,
            status_elapsed: parts.status_elapsed,
        }
    }
}

/// Resolve first, classify second. `classify` is unreachable when the provider
/// cannot be resolved, so a refusal is structurally incapable of producing a
/// verdict measured against fictional geometry.
pub fn with_resolved_rows<S, T, F>(
    source: &S,
    agent_type: &str,
    classify: F,
) -> Result<T, StatusBarRowsError>
where
    S: StatusBarRowsSource + ?Sized,
    F: FnOnce(&ClassifyInputBuilder) -> T,
{
    let builder = ClassifyInputBuilder::for_provider(source, agent_type)?;
    Ok(classify(&builder))
}

#[cfg(test)]
mod tests;
