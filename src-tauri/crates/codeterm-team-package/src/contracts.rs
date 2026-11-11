use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{
    payload_items, IconKind, ManagerSlot, Package, PackageCheckReport, PackageDiagnostic,
    PackageFormat, PackageLineage, PackageNetwork, PayloadKind, RoleStart, TeamRoleGroup,
    TemplatePayloadItem,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateRoleSummary {
    pub group: TeamRoleGroup,
    /// The manager slot id or the worker name.
    pub role: String,
    /// The manager display name, or the role id.
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub start: RoleStart,
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

/// A model/effort picker for a with_team manager the owner must choose for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ManagerModelPicker {
    pub slot: ManagerSlot,
    pub label: String,
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
    /// Choices for with_team managers beyond the pair's own Orchestrator and Planner fields.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub manager_models: BTreeMap<ManagerSlot, ManagerModelChoice>,
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
    pub group: TeamRoleGroup,
    pub role: String,
    pub state: TemplateItemState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ManagerStartFailure {
    pub slot: ManagerSlot,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum TemplateStartRefusal {
    ConsentRequired { reasons: Vec<ConsentReason> },
    InvalidPackage { diagnostics: Vec<PackageDiagnostic> },
    NotFound { template: TemplateRef },
    MissingInputs { inputs: Vec<String> },
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateExportRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<TemplateRef>,
    pub format: PackageFormat,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateExportResult {
    pub format: PackageFormat,
    pub check: PackageCheckReport,
    pub scrub: ScrubReport,
}

/// Start applies the opted-in payload; bake collects it back through the same mapping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PayloadApplyResult {
    pub applied: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<String>,
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
        }
    }
}

/// Every role with its display label and effective start, managers first.
pub fn role_summaries(package: &Package) -> Vec<TemplateRoleSummary> {
    package
        .document
        .roles()
        .into_iter()
        .map(|(group, role, declared)| {
            let slot = match group {
                TeamRoleGroup::Manager => ManagerSlot::parse(role),
                TeamRoleGroup::Worker => None,
            };
            TemplateRoleSummary {
                group,
                role: role.to_string(),
                label: declared.name.clone().unwrap_or_else(|| role.to_string()),
                description: declared.description.clone(),
                start: declared.effective_start(slot),
            }
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
    for (_, role, declared) in document.roles() {
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
    PayloadApplyResult::export_all(config)?;
    TemplateUnavailable::export_all(config)?;
    ModelDecision::export_all(config)?;
    Ok(())
}

#[cfg(test)]
mod tests;
