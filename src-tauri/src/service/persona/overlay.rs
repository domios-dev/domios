//! Disk-declared leaf kinds. An overlay may add a kind, a label, and layered
//! guidance; it can never mint capability or drop the coordination core.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use codeterm_types::agent::role_kind::builtins;
use parking_lot::RwLock;

use crate::multiplexer::types::AgentRole;
use crate::service::role_preset::{self, PresetOverlays};

/// Directory name under the instance data dir holding `*.toml` overlays.
pub(crate) const OVERLAY_DIR: &str = "role-presets";

/// Keys an overlay may declare.
const ALLOWED_KEYS: &[&str] = &["id", "label", "base", "guidance"];

/// Keys that would mint structural authority. Named explicitly so the rejection
/// says what was claimed instead of "unknown field".
const CAPABILITY_KEYS: &[&str] = &[
    "anchors_team",
    "anchoring",
    "can_spawn",
    "capability",
    "origin",
    "requires_parent",
    "spawns",
    "team_authority",
];

/// A structural needle from the text it guards. `worker_suffix` must keep every
/// one of these however an overlay layers on top of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CoreFragment {
    pub id: &'static str,
    pub needle: &'static str,
}

pub(crate) const CONTRACT_CORE: &[CoreFragment] = &[
    CoreFragment {
        id: "framework_rule",
        needle: "You are a Worker: a skilled colleague who owns this one task and its lane.",
    },
    CoreFragment {
        id: "report_protocol",
        needle: "codeterm agent report --status in_review",
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OverlayKind {
    pub id: String,
    pub label: String,
    pub guidance: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RejectionReason {
    Unreadable(String),
    Malformed(String),
    /// Builtin rows are embedded and non-overridable.
    BuiltinOverride {
        kind_id: String,
    },
    /// Declares a structural capability a runtime kind can never hold.
    CapabilityEscalation {
        claim: String,
    },
    /// Claims a base that renames, adopts into, or closes a Team.
    TeamAuthorityClaim {
        base: String,
    },
    /// The contract it claims would ship without a core fragment.
    MissingCore {
        fragment: &'static str,
    },
    EmptyGuidance,
}

impl RejectionReason {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Unreadable(_) => "unreadable",
            Self::Malformed(_) => "malformed",
            Self::BuiltinOverride { .. } => "builtin_override",
            Self::CapabilityEscalation { .. } => "capability_escalation",
            Self::TeamAuthorityClaim { .. } => "team_authority_claim",
            Self::MissingCore { .. } => "missing_core",
            Self::EmptyGuidance => "empty_guidance",
        }
    }

    pub(crate) fn detail(&self) -> String {
        match self {
            Self::Unreadable(error) | Self::Malformed(error) => error.clone(),
            Self::BuiltinOverride { kind_id } => format!("`{kind_id}` is a builtin role kind"),
            Self::CapabilityEscalation { claim } => format!("declares `{claim}`"),
            Self::TeamAuthorityClaim { base } => format!("claims the `{base}` base"),
            Self::MissingCore { fragment } => format!("drops the `{fragment}` core fragment"),
            Self::EmptyGuidance => "declares no guidance".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OverlayRejection {
    pub path: String,
    pub reason: RejectionReason,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct OverlayCatalog {
    kinds: BTreeMap<String, OverlayKind>,
    rejected: Vec<OverlayRejection>,
}

impl OverlayCatalog {
    pub(crate) fn empty() -> Self {
        Self::default()
    }

    pub(crate) fn kinds(&self) -> impl Iterator<Item = &OverlayKind> {
        self.kinds.values()
    }

    pub(crate) fn rejected(&self) -> &[OverlayRejection] {
        &self.rejected
    }

    /// Read every `*.toml` in `dir`. A rejected file is recorded and skipped;
    /// one bad overlay never blocks the rest.
    pub(crate) fn load(dir: &Path) -> Self {
        let mut catalog = Self::default();
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            // An absent directory is the normal state, not a rejection.
            Err(_) => return catalog,
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("toml"))
            .collect();
        paths.sort();
        for path in paths {
            let display = path.to_string_lossy().replace('\\', "/");
            match std::fs::read_to_string(&path) {
                Ok(text) => catalog.admit(&display, &text),
                Err(error) => {
                    catalog.reject(&display, RejectionReason::Unreadable(error.to_string()))
                }
            }
        }
        catalog
    }

    #[cfg(test)]
    pub(crate) fn from_sources(sources: &[(&str, &str)]) -> Self {
        let mut catalog = Self::default();
        for (path, text) in sources {
            catalog.admit(path, text);
        }
        catalog
    }

    fn reject(&mut self, path: &str, reason: RejectionReason) {
        self.rejected.push(OverlayRejection {
            path: path.to_string(),
            reason,
        });
    }

    fn admit(&mut self, path: &str, text: &str) {
        match parse(path, text) {
            Ok(kind) => {
                self.kinds.insert(kind.id.clone(), kind);
            }
            Err(reason) => self.reject(path, reason),
        }
    }
}

impl PresetOverlays for OverlayCatalog {
    fn label_for(&self, kind_id: &str) -> Option<&str> {
        self.kinds.get(kind_id).map(|kind| kind.label.as_str())
    }

    fn guidance_for(&self, kind_id: &str) -> Option<(&str, &str)> {
        self.kinds
            .get(kind_id)
            .map(|kind| (kind.guidance.as_str(), kind.path.as_str()))
    }
}

/// The base an overlay claims to layer on. Only a leaf base is admissible.
enum ClaimedBase {
    Leaf,
    Standalone,
}

fn parse(path: &str, text: &str) -> Result<OverlayKind, RejectionReason> {
    let table: toml::value::Table =
        toml::from_str(text).map_err(|error| RejectionReason::Malformed(error.to_string()))?;
    for key in table.keys() {
        if CAPABILITY_KEYS.contains(&key.as_str()) {
            return Err(RejectionReason::CapabilityEscalation { claim: key.clone() });
        }
        if !ALLOWED_KEYS.contains(&key.as_str()) {
            return Err(RejectionReason::Malformed(format!("unknown key `{key}`")));
        }
    }
    let id = string_key(&table, "id")?.unwrap_or_default();
    let id = id.trim();
    if id.is_empty() {
        return Err(RejectionReason::Malformed("missing `id`".to_string()));
    }
    if builtins::is_builtin(id) {
        return Err(RejectionReason::BuiltinOverride {
            kind_id: id.to_string(),
        });
    }
    let guidance = string_key(&table, "guidance")?.unwrap_or_default();
    let guidance = guidance.trim().to_string();
    if guidance.is_empty() {
        return Err(RejectionReason::EmptyGuidance);
    }
    let base = claimed_base(&table)?;
    let contract = match base {
        ClaimedBase::Leaf => leaf_contract(id, &guidance, path),
        // Honour the strongest claim the file makes: if it wants to own the
        // whole contract, judge that contract, not the layered one.
        ClaimedBase::Standalone => guidance.clone(),
    };
    if let Some(missing) = CONTRACT_CORE
        .iter()
        .find(|fragment| !contract.contains(fragment.needle))
    {
        return Err(RejectionReason::MissingCore {
            fragment: missing.id,
        });
    }
    let label = string_key(&table, "label")?
        .map(|label| label.trim().to_string())
        .filter(|label| !label.is_empty())
        .unwrap_or_else(|| id.to_string());
    Ok(OverlayKind {
        id: id.to_string(),
        label,
        guidance,
        path: path.to_string(),
    })
}

/// What this kind would actually ship: the resolver's own composition, so the
/// check reads the real contract rather than restating it.
fn leaf_contract(id: &str, guidance: &str, path: &str) -> String {
    struct Probe<'a> {
        id: &'a str,
        guidance: &'a str,
        path: &'a str,
    }
    impl PresetOverlays for Probe<'_> {
        fn label_for(&self, _kind_id: &str) -> Option<&str> {
            None
        }
        fn guidance_for(&self, kind_id: &str) -> Option<(&str, &str)> {
            (kind_id == self.id).then_some((self.guidance, self.path))
        }
    }
    role_preset::resolve_in(
        &Probe { id, guidance, path },
        Some(AgentRole::Worker),
        Some(id),
        None,
    )
    .map(|preset| preset.contract_guidance())
    .unwrap_or_default()
}

fn claimed_base(table: &toml::value::Table) -> Result<ClaimedBase, RejectionReason> {
    let Some(base) = string_key(table, "base")? else {
        return Ok(ClaimedBase::Leaf);
    };
    let base = base.trim();
    match base {
        "" | builtins::WORKER => Ok(ClaimedBase::Leaf),
        "none" | "standalone" => Ok(ClaimedBase::Standalone),
        _ => match builtins::lookup(base) {
            Some(descriptor) if descriptor.anchors_team() || descriptor.can_spawn() => {
                Err(RejectionReason::CapabilityEscalation {
                    claim: format!("base = \"{base}\""),
                })
            }
            Some(descriptor) if descriptor.has_team_authority() => {
                Err(RejectionReason::TeamAuthorityClaim {
                    base: base.to_string(),
                })
            }
            Some(_) => Ok(ClaimedBase::Leaf),
            None => Err(RejectionReason::Malformed(format!("unknown base `{base}`"))),
        },
    }
}

fn string_key(table: &toml::value::Table, key: &str) -> Result<Option<String>, RejectionReason> {
    match table.get(key) {
        None => Ok(None),
        Some(toml::Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(RejectionReason::Malformed(format!(
            "`{key}` must be a string"
        ))),
    }
}

pub(crate) fn overlay_dir() -> PathBuf {
    crate::paths::data_dir().join(OVERLAY_DIR)
}

static AMBIENT: std::sync::LazyLock<RwLock<Arc<OverlayCatalog>>> =
    std::sync::LazyLock::new(|| RwLock::new(Arc::new(OverlayCatalog::empty())));

/// The installed catalog. Empty until the service loads one, so unit tests and
/// fixtures never depend on the developer's home directory.
pub(crate) fn ambient() -> Arc<OverlayCatalog> {
    AMBIENT.read().clone()
}

pub(crate) fn install(catalog: OverlayCatalog) -> Arc<OverlayCatalog> {
    let catalog = Arc::new(catalog);
    *AMBIENT.write() = catalog.clone();
    catalog
}

/// Re-read the overlay directory. Adding a leaf kind needs this, never a build.
pub(crate) fn reload() -> Arc<OverlayCatalog> {
    install(OverlayCatalog::load(&overlay_dir()))
}
