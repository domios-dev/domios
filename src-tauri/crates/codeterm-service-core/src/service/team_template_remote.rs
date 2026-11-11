//! Peer hosting on existing mesh rails: the template-hosting capability gate, the role type
//! an origin composes into a remote Worker's brief, the keepawake lease an active hosted role
//! holds on its actual host, and the admission a mesh-origin template start passes before the
//! ordinary pair rail runs on this peer Main.

use std::collections::VecDeque;
use std::sync::{Arc, OnceLock};

use codeterm_team_package::contracts::{
    TemplateConsent, TemplatePreview, TemplateStartRefusal, TemplateStartRequest,
};
use codeterm_types::agent::AgentRole;
use codeterm_types::team::role_registration::PinnedPackage;
use parking_lot::Mutex;
use serde_json::Value;

use super::team_template_start::{admit, AdmittedTemplate, ConsentLedger};

/// Advertised by `identify` on a Domios that hosts template role types and peer starts.
pub const TEMPLATE_HOSTING_FEATURE: &str = "template_hosting";
/// The first Domios release that ships template hosting.
pub const TEMPLATE_HOSTING_MIN_VERSION: &str = "1.20.0";

fn release_triple(version: &str) -> Option<(u64, u64, u64)> {
    let version = semver::Version::parse(version.trim().trim_start_matches('v')).ok()?;
    Some((version.major, version.minor, version.patch))
}

/// A peer hosts a role type only when its own `identify` advertises the feature at a
/// supported version; the refusal names the version the owner must install.
pub fn hosting_gate(identify: &Value, peer_name: &str) -> Result<(), String> {
    let version = identify.get("version").and_then(Value::as_str);
    let advertised = identify
        .get("features")
        .and_then(Value::as_array)
        .is_some_and(|features| {
            features
                .iter()
                .any(|feature| feature.as_str() == Some(TEMPLATE_HOSTING_FEATURE))
        });
    let older = matches!(
        (
            version.and_then(release_triple),
            release_triple(TEMPLATE_HOSTING_MIN_VERSION),
        ),
        (Some(current), Some(required)) if current < required
    );
    if advertised && !older {
        return Ok(());
    }
    Err(format!(
        "peer_version_unsupported: peer '{peer_name}' runs Domios {} without template hosting; \
         hosting a role type needs Domios {TEMPLATE_HOSTING_MIN_VERSION} or newer. Update the \
         peer or choose another placement",
        version.unwrap_or("unknown")
    ))
}

/// A worker role type resolved on the origin; its guidance travels in the brief because
/// the peer holds no registry for the origin's Team.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedRole {
    pub kind: String,
    pub guidance: String,
}

/// Resolves `--role` against the origin Team's registry through the shared composition.
pub fn hosted_role(team_id: Option<&str>, role: &str) -> Result<HostedRole, String> {
    let role_type = super::team_roles::TeamRoleStore::global()
        .role_type(team_id, role)
        .map_err(|error| error.to_string())?;
    if !role_type.is_worker() {
        return Err(format!(
            "mesh dispatch launches a worker type; `{role}` is a manager type"
        ));
    }
    let guidance = super::persona::guidance_for(
        super::persona::Occasion::WorkerLaunchIdentity,
        &super::team_roles::registrations(team_id),
        Some(AgentRole::Worker),
        Some(role_type.kind_id.as_str()),
        None,
    );
    Ok(HostedRole {
        kind: role_type.kind_id,
        guidance,
    })
}

/// Places the role guidance after the brief's first paragraph, which stays the startup ACK.
pub fn compose_hosted_brief(brief: &str, role: &HostedRole) -> String {
    let guidance = role.guidance.trim();
    if guidance.is_empty() {
        return brief.to_string();
    }
    let section = format!("## Role type `{}`\n\n{guidance}", role.kind);
    match brief.split_once("\n\n") {
        Some((ack, rest)) => format!("{ack}\n\n{section}\n\n{rest}"),
        None => format!("{brief}\n\n{section}"),
    }
}

/// Holds this machine awake while a hosted role is active.
pub trait AwakePort: Send + Sync {
    /// Starts a bounded keepawake unless one is already active; true when this call started it.
    fn hold(&self) -> Result<bool, String>;
    fn release(&self) -> Result<(), String>;
}

#[derive(Default)]
struct LeaseState {
    holders: Vec<String>,
    port: Option<Arc<dyn AwakePort>>,
    /// Only a keepawake the leases started is stopped by them; an owner session is borrowed.
    owned: bool,
}

/// Active hosted roles on this machine. The first holder starts the keepawake and the last
/// release stops it; the port runs under the lease lock so acquire and release never interleave.
#[derive(Default)]
pub struct HostingLeases {
    state: Mutex<LeaseState>,
}

impl HostingLeases {
    pub fn global() -> &'static Self {
        static LEASES: OnceLock<HostingLeases> = OnceLock::new();
        LEASES.get_or_init(Self::default)
    }

    pub fn acquire(&self, holder: &str, port: Arc<dyn AwakePort>) -> Result<(), String> {
        let mut state = self.state.lock();
        if state.holders.iter().any(|held| held == holder) {
            return Ok(());
        }
        if state.holders.is_empty() {
            state.owned = port.hold()?;
            state.port = Some(port);
        }
        state.holders.push(holder.to_string());
        Ok(())
    }

    /// Moves a lease taken for a dispatch attempt onto the Worker tab that now holds it.
    pub fn transfer(&self, from: &str, to: &str) {
        let mut state = self.state.lock();
        if state.holders.iter().any(|held| held == to) {
            state.holders.retain(|held| held != from);
        } else if let Some(held) = state.holders.iter_mut().find(|held| *held == from) {
            *held = to.to_string();
        }
    }

    pub fn release(&self, holder: &str) -> Result<(), String> {
        let mut state = self.state.lock();
        let before = state.holders.len();
        state.holders.retain(|held| held != holder);
        if state.holders.len() == before || !state.holders.is_empty() {
            return Ok(());
        }
        let port = state.port.take();
        let owned = std::mem::take(&mut state.owned);
        match port {
            Some(port) if owned => port.release(),
            _ => Ok(()),
        }
    }

    pub fn holds(&self, holder: &str) -> bool {
        self.state.lock().holders.iter().any(|held| held == holder)
    }
}

/// Releases a closed tab's hosting lease; a tab without one is a no-op.
pub fn release_hosting_lease(tab_id: &str) {
    if let Err(error) = HostingLeases::global().release(tab_id) {
        log::warn!("[peer-hosting] keepawake release for {tab_id} failed: {error}");
    }
}

/// Secret bindings whose named entry this machine's own vault lacks. Only presence is
/// asked; values are never read here and never travel over the mesh.
pub fn unbound_secrets(
    request: &TemplateStartRequest,
    mut present: impl FnMut(&str) -> Result<bool, String>,
) -> Vec<String> {
    request
        .secret_bindings
        .iter()
        .filter(|(_, entry)| !entry.trim().is_empty())
        .filter(|(_, entry)| !present(entry.trim()).unwrap_or(false))
        .map(|(name, _)| format!("secret:{name}"))
        .collect()
}

/// Bounds the mesh-start decisions remembered as spent; the oldest is forgotten first.
const MAX_SPENT_DECISIONS: usize = 256;

/// A consent decision admits at most one mesh-origin start, so a repeated request never
/// starts a second Team from the same owner decision.
#[derive(Default)]
pub struct SpentDecisions {
    spent: Mutex<VecDeque<String>>,
}

impl SpentDecisions {
    pub fn global() -> &'static Self {
        static SPENT: OnceLock<SpentDecisions> = OnceLock::new();
        SPENT.get_or_init(Self::default)
    }

    fn spend(&self, decision_id: &str) -> bool {
        let mut spent = self.spent.lock();
        if spent.iter().any(|id| id == decision_id) {
            return false;
        }
        if spent.len() == MAX_SPENT_DECISIONS {
            spent.pop_front();
        }
        spent.push_back(decision_id.to_string());
        true
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PeerStartRefusal {
    /// The origin peer lacks Full mesh access to this machine.
    Forbidden,
    /// A mesh start always creates a fresh Team here; it never re-runs the origin's Team.
    OriginTeam,
    /// The owner decision was already used for another mesh start.
    DecisionSpent,
    Refused(TemplateStartRefusal),
}

impl PeerStartRefusal {
    pub fn message(&self) -> String {
        match self {
            Self::Forbidden => "peer template start requires Full mesh access".into(),
            Self::OriginTeam => {
                "peer template start creates a new Team on this machine; origin Team state never transfers".into()
            }
            Self::DecisionSpent => {
                "this consent decision already started a Team; the owner must preview again on this machine".into()
            }
            Self::Refused(refusal) => serde_json::to_string(refusal).unwrap_or_default(),
        }
    }
}

/// The checks a mesh-origin start passes on this peer before the ordinary pair rail runs:
/// Full access, a fresh Team, the 4a gates, an owner decision this machine issued (always,
/// even for a package that asks for nothing) spent once, and local vault entries by name.
pub fn admit_from_peer(
    request: &TemplateStartRequest,
    full_access: bool,
    ledger: &ConsentLedger,
    spent: &SpentDecisions,
    unbound: Vec<String>,
) -> Result<AdmittedTemplate, PeerStartRefusal> {
    if !full_access {
        return Err(PeerStartRefusal::Forbidden);
    }
    if request.team_id.is_some() {
        return Err(PeerStartRefusal::OriginTeam);
    }
    let admitted = admit(Some(request), ledger).map_err(PeerStartRefusal::Refused)?;
    let reasons = super::team_template_start::consent_reasons(
        &admitted.resolved.package,
        admitted.resolved.provenance,
    );
    let decision_id = request.decision_id.as_deref();
    if !ledger.admits(decision_id, &admitted.resolved.identity(), &reasons) {
        return Err(PeerStartRefusal::Refused(
            TemplateStartRefusal::ConsentRequired { reasons },
        ));
    }
    if !unbound.is_empty() {
        return Err(PeerStartRefusal::Refused(
            TemplateStartRefusal::MissingInputs { inputs: unbound },
        ));
    }
    if !decision_id.is_some_and(|id| spent.spend(id.trim())) {
        return Err(PeerStartRefusal::DecisionSpent);
    }
    Ok(admitted)
}

/// A local preview for a start a peer will request: the owner's decision is issued on this
/// machine even when the package asks for nothing, since a mesh start always needs one.
pub fn preview_for_mesh_start(
    svc: &super::ServiceContext,
    params: Option<&Value>,
) -> Result<TemplatePreview, super::team_template_start::TemplateCallError> {
    let mut preview = super::team_template_start::preview(svc, params)?;
    if preview.consent.is_none() {
        let identity = PinnedPackage {
            name: preview.form.template.name.clone(),
            version: preview.form.template.version.clone(),
        };
        preview.consent = Some(TemplateConsent {
            decision_id: ConsentLedger::global().issue(&identity, &[]),
            reasons: Vec::new(),
        });
    }
    Ok(preview)
}

#[cfg(test)]
#[path = "team_template_remote/tests.rs"]
mod tests;
