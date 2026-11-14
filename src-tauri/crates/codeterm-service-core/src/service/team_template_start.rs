//! Template list, preview and the gates every pair request passes before anything is created.
//! The pair primitive in codeterm-orchestration is the only start path; it calls `admit` here.

use std::collections::BTreeMap;

use codeterm_team_package::contracts::{
    declared_asks, input_fields, role_summaries, ManagerModelPicker, ModelDecision,
    PlacementProblem, RoleAuthority, TemplateConsent, TemplateList, TemplatePreview, TemplateRef,
    TemplateRoleSummary, TemplateRuntimeNeed, TemplateStartForm, TemplateStartRefusal,
    TemplateStartRequest, TemplateSummary, TemplateWhere,
};
use codeterm_team_package::{payload_items, RoleKind, RoleStart, StartMode, ORCHESTRATOR};
use serde_json::Value;

use super::model_decision::{
    decide_team_models, HarnessReadiness, TargetCatalogue, TeamModelInputs,
};
use super::ServiceContext;

mod consent;
mod resolve;

pub use consent::{consent_reasons, ConsentLedger};
pub use resolve::{
    builtin_templates, host_policy, refuse_host_owned_roles, resolve, ResolvedTemplate,
};

/// A template verb that could not answer: malformed parameters or a typed refusal.
#[derive(Debug, Clone, PartialEq)]
pub enum TemplateCallError {
    InvalidParams(String),
    Refused(TemplateStartRefusal),
}

impl From<TemplateStartRefusal> for TemplateCallError {
    fn from(refusal: TemplateStartRefusal) -> Self {
        Self::Refused(refusal)
    }
}

/// A resolved package that passed the input, secret and consent gates.
#[derive(Debug, Clone)]
pub struct AdmittedTemplate {
    pub resolved: ResolvedTemplate,
    /// Every declared input with its value or default; only declared keys survive.
    pub inputs: BTreeMap<String, String>,
    pub decision_id: Option<String>,
    /// Worker role type to the paired peer that hosts it.
    pub placements: BTreeMap<String, String>,
}

/// Builtins first, then the newest version of every template in My templates.
pub fn list(_svc: &ServiceContext) -> TemplateList {
    list_in(super::team_exchange::library::TemplateLibrary::global())
}

pub fn list_in(library: &super::team_exchange::library::TemplateLibrary) -> TemplateList {
    TemplateList {
        templates: builtin_templates()
            .map(|template| TemplateSummary::from_package(&template.package, template.provenance))
            .chain(library.list())
            .collect(),
    }
}

/// Resolves, gathers missing inputs and enforces consent before any state changes.
pub fn admit(
    request: Option<&TemplateStartRequest>,
    ledger: &ConsentLedger,
) -> Result<AdmittedTemplate, TemplateStartRefusal> {
    admit_bound(request, ledger, &[])
}

/// Bound vault entries this machine lacks, as `secret:<name>`; presence only, values never read.
pub async fn vault_unbound(
    svc: &ServiceContext,
    request: Option<&TemplateStartRequest>,
) -> Vec<String> {
    let Some(request) = request.filter(|request| !request.secret_bindings.is_empty()) else {
        return Vec::new();
    };
    let vault = svc.secrets.read().await;
    let mut present = Vec::new();
    for entry in request.secret_bindings.values() {
        let entry = entry.trim();
        if !entry.is_empty() && vault.has(entry).await.unwrap_or(false) {
            present.push(entry.to_string());
        }
    }
    super::team_template_remote::unbound_secrets(request, |entry| {
        Ok(present.iter().any(|name| name == entry))
    })
}

/// `admit`, where `unbound` names the bindings whose vault entry is absent on this machine.
pub fn admit_bound(
    request: Option<&TemplateStartRequest>,
    ledger: &ConsentLedger,
    unbound: &[String],
) -> Result<AdmittedTemplate, TemplateStartRefusal> {
    let resolved = resolve(request.map(|request| &request.template))?;
    let empty = BTreeMap::new();
    let given = request.map_or(&empty, |request| &request.inputs);
    let bindings = request.map_or(&empty, |request| &request.secret_bindings);
    let mut inputs = BTreeMap::new();
    let mut missing = Vec::new();
    for field in input_fields(&resolved.package) {
        let value = given
            .get(&field.key)
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or(field.default.clone());
        match value {
            Some(value) => {
                inputs.insert(field.key, value);
            }
            None if field.required => missing.push(field.key),
            None => {}
        }
    }
    for secret in &resolved.package.document.secrets {
        let bound = bindings
            .get(&secret.name)
            .is_some_and(|entry| !entry.trim().is_empty());
        if secret.is_required() && !bound {
            missing.push(format!("secret:{}", secret.name));
        }
    }
    for key in unbound {
        if !missing.contains(key) {
            missing.push(key.clone());
        }
    }
    if !missing.is_empty() {
        return Err(TemplateStartRefusal::MissingInputs { inputs: missing });
    }
    let placements = request.map_or(&empty, |request| &request.role_placements);
    worker_placements(&resolved, placements)?;
    let reasons = consent_reasons(&resolved.package, resolved.provenance);
    let decision_id = request.and_then(|request| request.decision_id.clone());
    if !reasons.is_empty() && !ledger.admits(decision_id.as_deref(), &resolved.identity(), &reasons)
    {
        return Err(TemplateStartRefusal::ConsentRequired { reasons });
    }
    Ok(AdmittedTemplate {
        placements: placements.clone(),
        resolved,
        inputs,
        decision_id: if reasons.is_empty() {
            None
        } else {
            decision_id
        },
    })
}

/// A start admitted on the owner's consent decision verifies that saved `name@version`.
/// Blocking: writes the My templates record.
pub fn record_consent(admitted: &AdmittedTemplate) {
    if admitted.decision_id.is_none()
        || admitted.resolved.provenance
            == codeterm_team_package::contracts::TemplateProvenance::Builtin
    {
        return;
    }
    let identity = admitted.resolved.identity();
    if let Err(error) = super::team_exchange::library::TemplateLibrary::global()
        .mark_verified(&identity.name, &identity.version)
    {
        log::warn!(
            "[template-start] {}@{} consent not recorded: {error}",
            identity.name,
            identity.version
        );
    }
}

fn placement_refusal(role: &str, peer: &str, problem: PlacementProblem) -> TemplateStartRefusal {
    TemplateStartRefusal::InvalidPlacement {
        role: role.to_string(),
        peer: peer.to_string(),
        problem,
    }
}

/// Only worker role types the package declares can run elsewhere; managers are the Team's home.
fn worker_placements(
    resolved: &ResolvedTemplate,
    placements: &BTreeMap<String, String>,
) -> Result<(), TemplateStartRefusal> {
    let document = &resolved.package.document;
    for (role, peer) in placements {
        match document.role(role).map(|declared| declared.kind) {
            None => return Err(placement_refusal(role, peer, PlacementProblem::UnknownRole)),
            Some(RoleKind::Manager) => {
                return Err(placement_refusal(role, peer, PlacementProblem::NotAWorker))
            }
            Some(RoleKind::Worker) => {}
        }
    }
    Ok(())
}

/// Each placement names a paired peer with Full access; the peer's own name is what dispatch uses.
pub fn resolve_placements(
    svc: &ServiceContext,
    admitted: &mut AdmittedTemplate,
) -> Result<(), TemplateStartRefusal> {
    if admitted.placements.is_empty() {
        return Ok(());
    }
    let handle = svc.try_ext::<crate::MeshStateHandle>();
    let mesh = handle.map(|handle| handle.lock());
    for (role, peer) in admitted.placements.iter_mut() {
        let paired = mesh.as_ref().and_then(|mesh| {
            crate::mesh::paired_resolve::resolve_paired_by_name(&mesh.paired_peers, peer.as_str())
                .ok()
                .map(|paired| {
                    (
                        paired.access_level == crate::mesh::types::AccessLevel::Full,
                        paired.instance_name.clone(),
                    )
                })
        });
        match paired {
            None => return Err(placement_refusal(role, peer, PlacementProblem::UnknownPeer)),
            Some((false, _)) => {
                return Err(placement_refusal(
                    role,
                    peer,
                    PlacementProblem::NotFullAccess,
                ))
            }
            Some((true, name)) => *peer = name,
        }
    }
    Ok(())
}

/// The managers the package starts with the Team, Orchestrator first, then in package order.
/// A curator runs on its schedule, never as a with_team member.
pub fn with_team_managers(resolved: &ResolvedTemplate) -> Vec<String> {
    let document = &resolved.package.document;
    let mut names: Vec<String> = document
        .roles_of(RoleKind::Manager)
        .filter(|(name, role)| {
            *name != codeterm_team_package::CURATOR
                && role.effective_start(name) == RoleStart::Mode(StartMode::WithTeam)
        })
        .map(|(name, _)| name.to_string())
        .collect();
    names.sort_by_key(|name| name.as_str() != ORCHESTRATOR);
    names
}

/// The pickers for with_team managers by role name, from the shared decisions of the
/// package's roles.
pub fn manager_pickers(
    resolved: &ResolvedTemplate,
    decisions: &BTreeMap<String, ModelDecision>,
) -> Vec<ManagerModelPicker> {
    let labels = role_summaries(&resolved.package);
    with_team_managers(resolved)
        .into_iter()
        .filter_map(|name| {
            let decision = decisions.get(&name)?.clone();
            let label = labels
                .iter()
                .find(|role| role.kind == RoleKind::Manager && role.role == name)
                .map_or_else(|| name.clone(), |role| role.label.clone());
            Some(ManagerModelPicker {
                authority: RoleAuthority::of(RoleKind::Manager, &name),
                role: name,
                label,
                decision,
            })
        })
        .collect()
}

/// Every role summary with its model from the same shared decisions; with_team managers carry theirs as pickers instead.
fn decided_roles(
    resolved: &ResolvedTemplate,
    decisions: &BTreeMap<String, ModelDecision>,
) -> Vec<TemplateRoleSummary> {
    let picked = with_team_managers(resolved);
    role_summaries(&resolved.package)
        .into_iter()
        .map(|mut role| {
            let has_picker = role.kind == RoleKind::Manager && picked.contains(&role.role);
            if !has_picker {
                role.model = decisions.get(&role.role).cloned();
            }
            role
        })
        .collect()
}

/// The package's required harnesses this machine lacks.
pub fn runtime_needs(
    resolved: &ResolvedTemplate,
    target: &TargetCatalogue,
) -> Option<TemplateWhere> {
    let needs: Vec<_> = resolved
        .requires_harnesses()
        .iter()
        .filter(|harness| target.readiness(harness) == HarnessReadiness::Absent)
        .map(|harness| TemplateRuntimeNeed::MissingHarness {
            harness: harness.clone(),
        })
        .collect();
    (!needs.is_empty()).then_some(TemplateWhere { needs })
}

/// The preview of one template on `target`; consent decisions are issued through `ledger`.
pub fn preview_on(
    template: &TemplateRef,
    target: &TargetCatalogue,
    ledger: &ConsentLedger,
) -> Result<TemplatePreview, TemplateStartRefusal> {
    let resolved = resolve(Some(template))?;
    let reasons = consent_reasons(&resolved.package, resolved.provenance);
    let consent = (!reasons.is_empty()).then(|| TemplateConsent {
        decision_id: ledger.issue(&resolved.identity(), &reasons),
        reasons,
    });
    let decisions = decide_team_models(
        &resolved.package.document,
        &TeamModelInputs::default(),
        target,
    );
    Ok(TemplatePreview {
        form: TemplateStartForm {
            template: super::team_exchange::library::TemplateLibrary::global()
                .summarize(&resolved.package, resolved.provenance),
            pickers: manager_pickers(&resolved, &decisions),
            roles: decided_roles(&resolved, &decisions),
            inputs: input_fields(&resolved.package),
        },
        where_needed: runtime_needs(&resolved, target),
        requires_harnesses: resolved.requires_harnesses().to_vec(),
        asks: declared_asks(&resolved.package),
        payload: payload_items(&resolved.package),
        warnings: resolved.warnings.clone(),
        consent,
        needs_project: resolved.package.document.project_need()
            == codeterm_team_package::ProjectNeed::Required,
    })
}

fn template_param(params: Option<&Value>) -> Result<TemplateRef, TemplateCallError> {
    let template = params
        .and_then(|params| params.get("template"))
        .ok_or_else(|| TemplateCallError::InvalidParams("`template` is required".into()))?;
    serde_json::from_value(template.clone())
        .map_err(|error| TemplateCallError::InvalidParams(format!("invalid `template`: {error}")))
}

/// Blocking: reads this machine's catalogue.
pub fn preview(
    svc: &ServiceContext,
    params: Option<&Value>,
) -> Result<TemplatePreview, TemplateCallError> {
    let template = template_param(params)?;
    Ok(preview_on(
        &template,
        &TargetCatalogue::local(svc),
        ConsentLedger::global(),
    )?)
}

#[cfg(test)]
#[path = "team_template_start/tests.rs"]
mod tests;
