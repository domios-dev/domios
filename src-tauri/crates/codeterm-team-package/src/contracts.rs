use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{
    payload_items, IconKind, ModelPreference, Package, PackageAuthor, PackageCheckReport,
    PackageDiagnostic, PackageFormat, PackageLineage, PackageLinks, PackageNetwork, PayloadKind,
    RoleKind, RoleStart, TemplatePayloadItem,
};

/// Names one template: a builtin or saved package, optionally pinned to a version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateRef {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TemplateProvenance {
    Builtin,
    Saved,
    /// Received from someone else: unverified, zero default grants.
    Imported,
}

/// The package icon as a data URL, rendered as an image and never as inline SVG DOM.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateIcon {
    pub media_type: String,
    pub data_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateSummary {
    pub name: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when: Vec<String>,
    pub provenance: TemplateProvenance,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<TemplateIcon>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived_from: Option<PackageLineage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<PackageAuthor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub links: Option<PackageLinks>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// How many worker roles the package declares.
    #[serde(default)]
    pub workers: u32,
    /// The first [`SUMMARY_NAME_LIMIT`] worker display names, in declaration order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub worker_names: Vec<String>,
    /// How many skills the package brings.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub skills: u32,
    /// The first [`SUMMARY_NAME_LIMIT`] skill names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skill_names: Vec<String>,
    /// The friend's display name when the template arrived through a share card.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub received_from: Option<String>,
    /// A start of this `name@version` passed the owner's consent on this machine.
    #[serde(default)]
    pub verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateRoleSummary {
    pub kind: RoleKind,
    /// The role name, the key in the package's `roles`.
    pub role: String,
    /// The manager display name, or the role id.
    pub label: String,
    pub authority: RoleAuthority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub start: RoleStart,
    /// The role's model decided on the previewing machine; with_team managers carry theirs as pickers instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelDecision>,
    /// The role whose model this one follows (`model: {same_as: <role>}`), so a client names it even when undecided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub same_as: Option<String>,
}

/// Bounds the names a template summary previews; the counts give the rest.
pub const SUMMARY_NAME_LIMIT: usize = 4;

fn is_zero(value: &u32) -> bool {
    *value == 0
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateInputField {
    pub key: String,
    pub description: String,
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ModelOption {
    /// `harness:model`.
    pub model: String,
    /// Present only when the provider declares a price.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ModelDecisionReason {
    FirstPreference,
    WithinListFallback,
    SameAs,
    NormalDefault,
    OwnerChoice,
    NeedsChoice,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModelBlockedReason {
    AbsentHarness {
        harness: String,
    },
    NotSignedIn {
        harness: String,
    },
    Quota {
        harness: String,
    },
    UnknownModel {
        model: String,
        suggestions: Vec<String>,
    },
    SameAsUnavailable {
        role: String,
    },
    /// The role sets an effort and the model offers no reasoning effort at all.
    NoReasoningEffort {
        model: String,
    },
    MissingTool {
        tool: String,
    },
    PreferencesExhausted,
}

/// A requested effort and the one applied; they differ only when the model cannot honour it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EffortDecision {
    pub requested: String,
    pub applied: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// One ranked model decision for one role on one machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ModelDecision {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choice: Option<ModelOption>,
    pub reason: ModelDecisionReason,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternatives: Vec<ModelOption>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked: Option<ModelBlockedReason>,
    /// An automatic fallback within the role's own list.
    pub amber: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<EffortDecision>,
}

/// The Domios authority a role carries; clients render it and never derive it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum RoleAuthority {
    /// A built-in manager name of kind manager keeps its Domios duties.
    Builtin,
    /// A custom manager or a worker: no Domios authority.
    None,
}

impl RoleAuthority {
    /// The one decision: authority follows a built-in manager name, and only for kind manager.
    pub fn of(kind: RoleKind, name: &str) -> Self {
        if kind == RoleKind::Manager && crate::is_builtin_manager(name) {
            Self::Builtin
        } else {
            Self::None
        }
    }
}

/// A model/effort picker for a with_team manager the owner must choose for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ManagerModelPicker {
    /// The manager's role name.
    pub role: String,
    pub label: String,
    pub authority: RoleAuthority,
    pub decision: ModelDecision,
}

/// The simple local start form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateStartForm {
    pub template: TemplateSummary,
    pub pickers: Vec<ManagerModelPicker>,
    pub roles: Vec<TemplateRoleSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<TemplateInputField>,
}

/// A real ask the owner sees before start; absent asks are never shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TemplateAsk {
    /// Empty `hosts` means any host.
    Network {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        hosts: Vec<String>,
    },
    RepoWrite,
    Shell,
    Browser,
    Secret {
        name: String,
        purpose: String,
        required: bool,
    },
    Tool {
        role: String,
        tool: String,
    },
    Script {
        item: String,
    },
}

/// A host-side dependency the home machine lacks; distinct from a malformed package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TemplateRuntimeNeed {
    MissingHarness { harness: String },
    MissingTool { tool: String },
}

/// Present only when home cannot host the package as declared.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateWhere {
    pub needs: Vec<TemplateRuntimeNeed>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ConsentReason {
    UnverifiedPackage,
    NewGrants,
}

/// The owner decision a start must carry back; the server records and enforces it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateConsent {
    pub decision_id: String,
    pub reasons: Vec<ConsentReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplatePreview {
    pub form: TemplateStartForm,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub where_needed: Option<TemplateWhere>,
    /// The package's `requires.harnesses`, whether or not this machine has them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires_harnesses: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub asks: Vec<TemplateAsk>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub payload: Vec<TemplatePayloadItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<PackageDiagnostic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consent: Option<TemplateConsent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ManagerModelChoice {
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
}

/// The `template` carried by an orchestration pair request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateStartRequest {
    pub template: TemplateRef,
    /// An existing Team to re-run the start on; missing managers and registrations are added.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, String>,
    /// Payload item ids the owner opted out of.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub opt_out: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_id: Option<String>,
    /// Secret name to the local vault entry that supplies it; values never travel.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub secret_bindings: BTreeMap<String, String>,
    /// Choices for with_team managers by role name, beyond the pair's own Orchestrator and Planner fields.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub manager_models: BTreeMap<String, ManagerModelChoice>,
    /// Worker role type to the paired peer Main that hosts it; managers always stay home.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub role_placements: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TemplateItemState {
    Created,
    AlreadyPresent,
    /// Registered from another version of the package; reported, never overwritten.
    PinnedToOtherVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateStartItem {
    pub kind: RoleKind,
    pub role: String,
    pub state: TemplateItemState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ManagerStartFailure {
    /// The manager's role name.
    pub role: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum TemplateStartRefusal {
    ConsentRequired {
        reasons: Vec<ConsentReason>,
    },
    InvalidPackage {
        diagnostics: Vec<PackageDiagnostic>,
    },
    NotFound {
        template: TemplateRef,
    },
    MissingInputs {
        inputs: Vec<String>,
    },
    /// A `role_placements` entry the start cannot honour; nothing was created.
    InvalidPlacement {
        role: String,
        peer: String,
        problem: PlacementProblem,
    },
    /// Domios runs scheduled rounds only for the curator; a schedule on another role is valid
    /// ATP that this host does not start.
    ScheduleUnsupportedRole {
        role: String,
    },
    /// A package role under a name the host owns (`watcher`, `general_agent`).
    HostOwnedRole {
        role: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PlacementProblem {
    /// The package declares no role of that name.
    UnknownRole,
    /// Managers are the Team's home and never move.
    NotAWorker,
    UnknownPeer,
    /// Hosting a role needs Full mesh access on that peer.
    NotFullAccess,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum TemplateStartResult {
    Started {
        team_id: String,
        managers: Vec<TemplateStartItem>,
        registrations: Vec<TemplateStartItem>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        warnings: Vec<PackageDiagnostic>,
    },
    /// Manager models the owner must pick before anything starts.
    NeedsChoice {
        choices: Vec<ManagerModelPicker>,
    },
    /// A manager failed to start; returned to the caller, never retried silently.
    ManagerFailed {
        failure: ManagerStartFailure,
    },
    Refused {
        refusal: TemplateStartRefusal,
    },
}

/// The code a non-started result carries when it travels as an error message.
pub const TEMPLATE_START_ERROR: &str = "template_start";

impl TemplateStartResult {
    /// `template_start: <json>`; the typed JSON closes the message so every transport keeps it.
    pub fn error_message(&self) -> String {
        format!(
            "{TEMPLATE_START_ERROR}: {}",
            serde_json::to_string(self).unwrap_or_default()
        )
    }

    pub fn from_error_message(message: &str) -> Option<Self> {
        let json = message
            .strip_prefix(TEMPLATE_START_ERROR)?
            .strip_prefix(": ")?;
        serde_json::from_str(json).ok()
    }

    /// The stable CLI error code a non-started outcome answers with; a started Team has none.
    pub fn error_code(&self) -> Option<&'static str> {
        Some(match self {
            Self::Started { .. } => return None,
            Self::NeedsChoice { .. } => "invalid_argument",
            Self::ManagerFailed { .. } => "startup_failed",
            Self::Refused { refusal } => match refusal {
                TemplateStartRefusal::ConsentRequired { .. } => "refused",
                TemplateStartRefusal::NotFound { .. } => "not_found",
                TemplateStartRefusal::InvalidPackage { .. }
                | TemplateStartRefusal::MissingInputs { .. }
                | TemplateStartRefusal::InvalidPlacement { .. }
                | TemplateStartRefusal::ScheduleUnsupportedRole { .. }
                | TemplateStartRefusal::HostOwnedRole { .. } => "invalid_argument",
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateList {
    pub templates: Vec<TemplateSummary>,
}

/// Best matches first; the fallback (the builtin default) is always offered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateRecall {
    pub matches: Vec<TemplateSummary>,
    pub fallback: TemplateSummary,
    pub recall_available: bool,
}

/// What an export removed or generalized.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ScrubReport {
    pub machines_removed: u32,
    pub paths_generalized: u32,
    pub secret_values_removed: u32,
    pub secret_asks_added: u32,
    pub history_removed: u32,
    /// The secret names kept as asks; their values never travel.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secret_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateExportRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<TemplateRef>,
    pub format: PackageFormat,
    /// Absolute destination; without it the package lands in the exports folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateExportResult {
    pub format: PackageFormat,
    pub check: PackageCheckReport,
    pub scrub: ScrubReport,
    /// Where the package was written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The exact `team.yaml` text, for review before sharing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yaml: Option<String>,
}

/// Bake a live Team: the package identity the owner chose plus the coordinator's runbook.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateDistillRequest {
    pub team_id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<PackageAuthor>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when: Vec<String>,
    /// Written into the orchestrator's instructions; there is no top-level brief.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runbook: Option<String>,
    /// Payload item ids (from the Includes list) the owner leaves out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub opt_out: Vec<String>,
    #[serde(default = "default_format")]
    pub format: PackageFormat,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out: Option<String>,
    /// Save the checked package to My templates.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub save: bool,
    /// Review only: return the exact YAML and Includes list without writing anything.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dry_run: bool,
}

fn default_format() -> PackageFormat {
    PackageFormat::Yaml
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateDistillResult {
    /// The exact `team.yaml` text.
    pub yaml: String,
    /// Everything the package brings, each with its opt-out id.
    pub includes: Vec<TemplatePayloadItem>,
    pub check: PackageCheckReport,
    pub scrub: ScrubReport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved: Option<TemplateSummary>,
}

/// What changes between the saved version of a template and an incoming one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateUpdateDiff {
    pub name: String,
    pub from_version: String,
    pub to_version: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles_added: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles_removed: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles_changed: Vec<String>,
    /// Roles kept by name whose kind changed; each is also listed in `roles_changed`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles_kind_changed: Vec<RoleKindChange>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub payload_added: Vec<TemplateDiffItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub payload_removed: Vec<TemplateDiffItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub asks_added: Vec<TemplateAsk>,
}

/// A role an update keeps by name but moves to another kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RoleKindChange {
    pub role: String,
    pub from: RoleKind,
    pub to: RoleKind,
}

/// A payload item an update adds or removes: its opt-out id and the label the owner reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateDiffItem {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateImportResult {
    pub template: TemplateSummary,
    pub check: PackageCheckReport,
    /// Present when My templates already holds another version; active Teams keep theirs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update: Option<TemplateUpdateDiff>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateDeleteRequest {
    pub template: TemplateRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateDeleteResult {
    /// The `name@version` entries removed from My templates.
    pub removed: Vec<String>,
}

/// Read one text inside a builtin or saved package: a file by its package-relative `path`,
/// or a role's instructions by `role`. Exactly one of the two is set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateFileRequest {
    pub template: TemplateRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

/// One package text, for review before the owner keeps it; nothing runs it. A role's
/// instructions answer with the file they live in (`team.yaml` when inline) and the role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateFileText {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub text: String,
}

/// What an origin asks a peer Main to start; the pair fields are the peer's own choices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplatePeerStartAsk {
    pub template: TemplateStartRequest,
    #[serde(default)]
    pub orchestrator_agent: String,
    #[serde(default)]
    pub orchestrator_model: Option<String>,
    #[serde(default)]
    pub planner_agent: String,
    #[serde(default)]
    pub planner_model: Option<String>,
}

/// Start a template on a paired peer Main; the peer's owner answers there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplatePeerStartRequest {
    pub peer: String,
    pub start: TemplatePeerStartAsk,
    /// The tab that receives the outcome once the peer's owner answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller_tab_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TemplatePeerStartState {
    AwaitingOwner,
}

/// The peer asked its owner; nothing has started until they tap Start here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplatePeerStartAccepted {
    pub state: TemplatePeerStartState,
    pub peer_id: String,
    pub peer_name: String,
    pub request_id: String,
    pub notification_id: String,
}

/// Send a checked package to a paired peer Main's My templates over the mesh.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateTransferRequest {
    pub template: TemplateRef,
    pub peer: String,
    #[serde(default = "default_transfer_format")]
    pub format: PackageFormat,
}

fn default_transfer_format() -> PackageFormat {
    PackageFormat::Team
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TemplateShareState {
    Unverified,
    Previewed,
    Dismissed,
}

/// The Neighborhood chat card for a received `.team.yaml` or `.team` attachment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateShareCard {
    pub file_id: String,
    pub template: TemplateSummary,
    pub roles: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub asks: Vec<TemplateAsk>,
    pub state: TemplateShareState,
}

/// A card action: Preview validates and adds the package to My templates; Dismiss drops the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TemplateShareAction {
    Preview,
    Dismiss,
}

/// Start applies the opted-in payload; bake collects it back through the same mapping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PayloadApplyResult {
    pub applied: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<String>,
    /// Accepted items that land later: Blueprints once the Orchestrator is live, skills once
    /// the Team's plan binds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deferred: Vec<String>,
    /// Items that failed; every other item was still applied.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed: Vec<PayloadItemFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PayloadItemFailure {
    pub id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TemplateOperation {
    List,
    Preview,
    Start,
    Recall,
    Distill,
    Export,
    Import,
    ApplyPayload,
    CollectPayload,
    RemoteStart,
    ModelDecision,
    Delete,
    Transfer,
    Share,
}

/// What a registered-but-unimplemented template operation answers; it never mutates state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateUnavailable {
    pub code: String,
    pub operation: TemplateOperation,
    pub message: String,
}

pub const TEMPLATE_UNAVAILABLE: &str = "template_unavailable";

impl TemplateUnavailable {
    pub fn new(operation: TemplateOperation) -> Self {
        let name = serde_json::to_value(operation)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_default();
        Self {
            code: TEMPLATE_UNAVAILABLE.to_string(),
            operation,
            message: format!("template {name} is not available in this Domios build yet"),
        }
    }
}

impl fmt::Display for TemplateUnavailable {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let triple = (u32::from(chunk[0]) << 16)
            | (u32::from(chunk.get(1).copied().unwrap_or(0)) << 8)
            | u32::from(chunk.get(2).copied().unwrap_or(0));
        for position in 0..4 {
            if position <= chunk.len() {
                let index = (triple >> (18 - 6 * position)) & 0x3f;
                out.push(char::from(BASE64[index as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

impl TemplateIcon {
    pub fn new(kind: IconKind, bytes: &[u8]) -> Self {
        Self {
            media_type: kind.media_type().to_string(),
            data_url: format!("data:{};base64,{}", kind.media_type(), base64(bytes)),
        }
    }
}

impl TemplateSummary {
    pub fn from_package(package: &Package, provenance: TemplateProvenance) -> Self {
        let document = &package.document;
        let worker_names = document
            .roles_of(RoleKind::Worker)
            .take(SUMMARY_NAME_LIMIT)
            .map(|(name, role)| role.name.clone().unwrap_or_else(|| name.to_string()))
            .collect();
        let skills: Vec<String> = payload_items(package)
            .into_iter()
            .filter(|item| item.kind == PayloadKind::Skill)
            .map(|item| item.label)
            .collect();
        Self {
            name: document.name.clone(),
            version: document.version.clone(),
            title: document.title.clone(),
            description: document.description.clone(),
            when: document.when.clone(),
            provenance,
            icon: package
                .icon()
                .map(|(kind, bytes)| TemplateIcon::new(kind, bytes)),
            derived_from: document.derived_from.clone(),
            authors: document.authors.clone(),
            links: document.links.clone(),
            license: document.license.clone(),
            workers: u32::try_from(document.roles_of(RoleKind::Worker).count()).unwrap_or(u32::MAX),
            worker_names,
            skills: u32::try_from(skills.len()).unwrap_or(u32::MAX),
            skill_names: skills.into_iter().take(SUMMARY_NAME_LIMIT).collect(),
            received_from: None,
            verified: false,
        }
    }
}

/// Every role with its display label and effective start, managers first, each kind in package order.
pub fn role_summaries(package: &Package) -> Vec<TemplateRoleSummary> {
    let document = &package.document;
    document
        .roles_of(RoleKind::Manager)
        .chain(document.roles_of(RoleKind::Worker))
        .map(|(role, declared)| TemplateRoleSummary {
            kind: declared.kind,
            role: role.to_string(),
            label: declared.name.clone().unwrap_or_else(|| role.to_string()),
            authority: RoleAuthority::of(declared.kind, role),
            description: declared.description.clone(),
            start: declared.effective_start(role),
            model: None,
            same_as: match &declared.model {
                Some(ModelPreference::SameAs { same_as }) => Some(same_as.clone()),
                _ => None,
            },
        })
        .collect()
}

pub fn input_fields(package: &Package) -> Vec<TemplateInputField> {
    package
        .document
        .inputs
        .iter()
        .map(|(key, input)| TemplateInputField {
            key: key.to_string(),
            description: input.description().to_string(),
            required: input.required(),
            default: input.default_value().map(str::to_string),
            choices: input.choices().to_vec(),
        })
        .collect()
}

/// The asks a package declares; the preview shows only these.
pub fn declared_asks(package: &Package) -> Vec<TemplateAsk> {
    let document = &package.document;
    let mut asks = Vec::new();
    if let Some(permissions) = &document.permissions {
        match &permissions.network {
            Some(PackageNetwork::Any(true)) => {
                asks.push(TemplateAsk::Network { hosts: Vec::new() })
            }
            Some(PackageNetwork::Hosts(hosts)) if !hosts.is_empty() => {
                asks.push(TemplateAsk::Network {
                    hosts: hosts.clone(),
                })
            }
            _ => {}
        }
        for (granted, ask) in [
            (permissions.repo_write, TemplateAsk::RepoWrite),
            (permissions.shell, TemplateAsk::Shell),
            (permissions.browser, TemplateAsk::Browser),
        ] {
            if granted == Some(true) {
                asks.push(ask);
            }
        }
    }
    for secret in &document.secrets {
        asks.push(TemplateAsk::Secret {
            name: secret.name.clone(),
            purpose: secret.purpose.clone(),
            required: secret.is_required(),
        });
    }
    for (role, declared) in document.roles() {
        for tool in &declared.tools {
            asks.push(TemplateAsk::Tool {
                role: role.to_string(),
                tool: tool.clone(),
            });
        }
    }
    for item in payload_items(package) {
        if item.executable && matches!(item.kind, PayloadKind::File | PayloadKind::Skill) {
            asks.push(TemplateAsk::Script { item: item.id });
        }
    }
    asks
}

/// Writes every contract and document type through ts-rs, dependencies included.
pub fn export_typescript(config: &ts_rs::Config) -> Result<(), ts_rs::ExportError> {
    crate::TeamDocument::export_all(config)?;
    crate::PackageCheckReport::export_all(config)?;
    PackageFormat::export_all(config)?;
    TemplateList::export_all(config)?;
    TemplatePreview::export_all(config)?;
    TemplateStartRequest::export_all(config)?;
    TemplateStartResult::export_all(config)?;
    TemplateRecall::export_all(config)?;
    TemplateExportRequest::export_all(config)?;
    TemplateExportResult::export_all(config)?;
    TemplateDistillRequest::export_all(config)?;
    TemplateDistillResult::export_all(config)?;
    TemplateImportResult::export_all(config)?;
    TemplateDeleteRequest::export_all(config)?;
    TemplateDeleteResult::export_all(config)?;
    TemplateTransferRequest::export_all(config)?;
    TemplateFileRequest::export_all(config)?;
    TemplateFileText::export_all(config)?;
    TemplatePeerStartRequest::export_all(config)?;
    TemplatePeerStartAccepted::export_all(config)?;
    TemplateShareCard::export_all(config)?;
    TemplateShareAction::export_all(config)?;
    PayloadApplyResult::export_all(config)?;
    PayloadItemFailure::export_all(config)?;
    TemplateUnavailable::export_all(config)?;
    ModelDecision::export_all(config)?;
    Ok(())
}

#[cfg(test)]
mod tests;
