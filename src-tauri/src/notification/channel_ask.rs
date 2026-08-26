//! Carrying a question's options over a push channel, and addressing one
//! offered choice from the other end.
//!
//! A channel never learns what a label means and never answers anything. It is
//! handed an opaque token per option and hands whichever token was tapped back
//! to the daemon, which resolves it against the record and then walks the same
//! answer path an in-app tap walks. There is one answer mechanism, and this is
//! an address for it rather than a second one.

use codeterm_types::notification::OfferedChoice;

/// The token prefix. Short on purpose: a channel's callback payload is the
/// tightest budget anywhere on this path.
const TOKEN_PREFIX: &str = "na";
const TOKEN_SEP: char = ':';

/// One option as a channel carries it: what to draw, and the opaque token to
/// hand back when it is tapped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelChoice {
    pub label: String,
    pub token: String,
}

/// One rendered notification as a channel receives it. The text is what every
/// channel has always been handed; the options ride alongside it, so a channel
/// that cannot draw them simply carries the text it always did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PushMessage {
    pub text: String,
    pub choices: Vec<ChannelChoice>,
    /// The record this renders. A channel tags its send with it so the
    /// placement that comes back names the question it belongs to; the channel
    /// echoes it and never reads it.
    pub record: String,
}

impl PushMessage {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            choices: Vec::new(),
            record: String::new(),
        }
    }

    pub fn offers_choices(&self) -> bool {
        !self.choices.is_empty()
    }
}

/// The offered labels of one action set, in the order the caller named them.
/// Every side of this path reads the set through here, so a token's position
/// and the label it resolves to cannot disagree.
pub fn offered_labels<A: OfferedChoice>(actions: &[A]) -> Vec<&str> {
    actions
        .iter()
        .filter_map(OfferedChoice::answer_label)
        .collect()
}

/// The label at one offered position. A position off the end resolves to
/// nothing rather than to a neighbour.
pub fn label_at<A: OfferedChoice>(actions: &[A], index: usize) -> Option<&str> {
    offered_labels(actions).get(index).copied()
}

/// The options a channel carries for one record.
pub fn choices_for<A: OfferedChoice>(notification_id: &str, actions: &[A]) -> Vec<ChannelChoice> {
    offered_labels(actions)
        .into_iter()
        .enumerate()
        .map(|(index, label)| ChannelChoice {
            label: label.to_string(),
            token: encode(notification_id, index),
        })
        .collect()
}

/// Which offered position of which record a tapped token names. Deliberately
/// not a label: a label is unbounded and a callback payload is not, and the
/// record is what decides which labels it ever offered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TappedChoice {
    pub notification_id: String,
    pub index: usize,
}

pub fn encode(notification_id: &str, index: usize) -> String {
    format!("{TOKEN_PREFIX}{TOKEN_SEP}{notification_id}{TOKEN_SEP}{index}")
}

/// Read a token back. Anything that is not one answers `None`, so ordinary
/// channel traffic is never mistaken for an answer.
pub fn decode(raw: &str) -> Option<TappedChoice> {
    let rest = raw
        .trim()
        .strip_prefix(TOKEN_PREFIX)?
        .strip_prefix(TOKEN_SEP)?;
    let (notification_id, index) = rest.rsplit_once(TOKEN_SEP)?;
    if notification_id.is_empty() {
        return None;
    }
    Some(TappedChoice {
        notification_id: notification_id.to_string(),
        index: index.parse().ok()?,
    })
}

#[cfg(test)]
mod tests;
