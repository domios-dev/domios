//! THE AUTHORITY RULE.
//!
//! Where a notification may go is the user's answer, never an agent's. An
//! agent that could switch a desktop toast or a push channel on by itself
//! would grant itself a capability the user never saw, so nothing an agent
//! reaches unattended may change one. An agent may OFFER, and the offer
//! applies on the tap — which is why the only constructor of the proof below
//! takes a first-and-only recorded answer.

use codeterm_types::config::schema::NotificationConfig;
use codeterm_types::notification::{NotifyDestination, NotifyLevel, OfferedChoice};
use serde::Serialize;

use super::answer::AnswerAdmission;
use super::channel_ask;

/// The config subtree that decides where a notification may go. Everything
/// under it is a surface decision; nothing outside it is.
pub const SURFACE_ROOT: &str = "notifications";

/// Whether one config path decides a notification surface.
pub fn is_surface_path(key: &str) -> bool {
    let key = key.trim();
    key == SURFACE_ROOT || key.starts_with(&format!("{SURFACE_ROOT}."))
}

/// The rule, as one refusal: NOTHING unattended may change where a notification
/// goes, whichever surface the key names.
///
/// There is deliberately no "but a tap may" arm here, and no authority argument
/// that could select one. A tap does not reach this function at all — it reaches
/// [`apply`], which cannot be called without a [`TapProof`]. An arm for the
/// permitted case would be a second answer to "may this change", and a second
/// answer is a second thing to get wrong.
pub fn admit_unattended_change(key: &str) -> Result<(), String> {
    if !is_surface_path(key) {
        return Ok(());
    }
    Err(format!(
        "'{}' decides where notifications go and changes only on a tapped answer; offer it instead of setting it",
        key.trim()
    ))
}

/// Proof that a user tapped an offered label. The field is private and the one
/// constructor below is the only way to obtain it, so no caller can conjure a
/// surface change out of its own judgement.
#[derive(Debug)]
pub struct TapProof(());

/// A first-and-only recording yields the proof. A repeat tap, an unoffered
/// label and a dead asking tab do not: an offer applies once, on the tap that
/// answered it.
pub fn tap_proof(admission: &AnswerAdmission) -> Option<TapProof> {
    matches!(admission, AnswerAdmission::Recorded(_)).then_some(TapProof(()))
}

/// What an offer may turn on. Core owns the set and spells no channel: a
/// channel is the service id the registry resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "surface", rename_all = "snake_case")]
pub enum SurfaceTarget {
    /// The OS notification centre.
    Desktop,
    /// One push channel, addressed by its service id.
    Channel { service: String },
}

impl SurfaceTarget {
    /// The grid column this surface is. Whether a level reaches anywhere is one
    /// question with one home - a cell of the levels-by-destinations grid - so
    /// an offer names a column and never a switch of its own.
    pub fn column(&self) -> NotifyDestination<'_> {
        match self {
            Self::Desktop => NotifyDestination::Desktop,
            Self::Channel { service } => NotifyDestination::Channel(service),
        }
    }
}

/// An agent's offer to be louder: the surface it would turn on, and the one
/// offered label that turns it on. Daemon-constructed — no `Deserialize`, so
/// no payload on any transport can mint one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SurfaceOffer {
    #[serde(flatten)]
    pub target: SurfaceTarget,
    pub accept: String,
}

impl SurfaceOffer {
    /// Whether this tapped label is the one the offer applies on. Compared the
    /// way the answer record already compares an offered label, so the accept
    /// and the offer cannot drift apart.
    pub fn accepted_by(&self, label: &str) -> bool {
        self.accept == label
    }
}

/// Adopt a caller's offer. The accepting label must be one the very same
/// notification offers, so an offer can never apply on a label no surface will
/// ever draw. The choice set itself was already vetted by the shared
/// validator; this only ties the offer to it.
pub fn adopt_offer<A: OfferedChoice>(
    target: SurfaceTarget,
    accept: &str,
    actions: &[A],
) -> Result<SurfaceOffer, String> {
    let accept = accept.trim();
    if !channel_ask::offered_labels(actions)
        .iter()
        .any(|label| *label == accept)
    {
        return Err(format!(
            "The accepting label '{accept}' is not one of this notification's choices"
        ));
    }
    Ok(SurfaceOffer {
        target,
        accept: accept.to_string(),
    })
}

/// What one offer changes, and the whole of it: ONE cell of the grid - the
/// offered column, on the row of the level the user was actually shown.
///
/// `level` is read off the record the tap answered, never supplied alongside
/// it, because that is the whole proportionality argument: the question
/// demonstrated one kind of notification, and the tap grants that kind. Opening
/// every level would let a tap on one `action_needed` question route routine
/// progress to a push channel the user was never shown.
///
/// The desktop master switch moves with the desktop cell because the two are
/// ANDed at the gate: opening the cell alone would change nothing the user
/// could observe, which is a silent no-op wearing the shape of a grant.
pub fn apply(
    _proof: TapProof,
    cfg: &mut NotificationConfig,
    target: &SurfaceTarget,
    level: NotifyLevel,
) {
    let column = target.column();
    if let Some(row) = cfg
        .agent_levels
        .iter_mut()
        .find(|row| NotifyLevel::parse(&row.level) == Some(level))
    {
        row.set(column, true);
    }
    if matches!(target, SurfaceTarget::Desktop) {
        cfg.desktop = true;
    }
}
