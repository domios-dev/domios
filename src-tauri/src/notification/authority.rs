//! THE AUTHORITY RULE.
//!
//! Where a notification may go is the user's answer, never an agent's. An
//! agent that could switch a desktop toast or a push channel on by itself
//! would grant itself a capability the user never saw, so nothing an agent
//! reaches unattended may change one. An agent may OFFER, and the offer
//! applies on the tap — which is why the only constructor of the proof below
//! takes a first-and-only recorded answer.

use codeterm_types::config::schema::NotificationConfig;
use codeterm_types::notification::OfferedChoice;
use serde::Serialize;

use super::answer::AnswerAdmission;
use super::channel_ask;

/// The config subtree that decides where a notification may go. Everything
/// under it is a surface decision; nothing outside it is.
pub const SURFACE_ROOT: &str = "notifications";

/// Who is asking to change a surface. Exactly one of these may widen one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeAuthority {
    /// The user tapped an offered label on a question the daemon asked.
    TappedAnswer,
    /// Anything an agent reaches on its own: its own config write, a bridge
    /// command, a scheduled job, a relayed call from a peer.
    Unattended,
}

/// Whether one config path decides a notification surface.
pub fn is_surface_path(key: &str) -> bool {
    let key = key.trim();
    key == SURFACE_ROOT || key.starts_with(&format!("{SURFACE_ROOT}."))
}

/// The rule, as one refusal. What may change without a tap is stated here and
/// nowhere else: nothing under the surface root, whichever surface it names.
pub fn admit_change(authority: ChangeAuthority, key: &str) -> Result<(), String> {
    if !is_surface_path(key) {
        return Ok(());
    }
    match authority {
        ChangeAuthority::TappedAnswer => Ok(()),
        ChangeAuthority::Unattended => Err(format!(
            "'{}' decides where notifications go and changes only on a tapped answer; offer it instead of setting it",
            key.trim()
        )),
    }
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
    /// One push channel's opt-in, addressed by its service id.
    Channel { service: String },
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

/// What one offer changes, and the whole of it. Exactly one field moves, which
/// is what makes the blast radius statable rather than implied.
pub fn apply(_proof: TapProof, cfg: &mut NotificationConfig, target: &SurfaceTarget) {
    match target {
        SurfaceTarget::Desktop => cfg.desktop = true,
        SurfaceTarget::Channel { service } => {
            cfg.channels.entry(service.clone()).or_default().enabled = true;
        }
    }
}
