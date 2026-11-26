use std::path::{Path, PathBuf};

use codeterm_util::persisted_state::{Loaded, StateFile};
use serde::{Deserialize, Serialize};

use crate::config::schema::AppConfig;
use crate::mem::{MemKind, MemManager, MemScope, NewMemItem, WakeUpOptions};

use super::filter::{projection_digest, sanitize_projection};
use super::types::{
    PortablePreferences, ProviderSignInCardStatus, WarmBootstrapAppliedCounts,
    WarmBootstrapApplyResult, WarmBootstrapProjection, WarmBootstrapRejectReason,
    WarmBootstrapStatus, MAX_PROJECTION_BYTES, WARM_BOOTSTRAP_BUDGET_SECS,
    WARM_BOOTSTRAP_SCHEMA_VERSION,
};

const WARM_BOOTSTRAP_STATE_FILE: &str = "cloud_warm_bootstrap.json";
const RECOVERY_STATE_FILE: &str = "cloud_recovery_state.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WarmBootstrapState {
    pub schema_version: u8,
    pub target_installation_id: String,
    pub account_id: String,
    pub owner_installation_id: String,
    pub creation_generation: String,
    pub applied_at_ms: u64,
    pub outcome: WarmBootstrapStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_revision: Option<String>,
    pub projection_digest: String,
    pub readable_owner_context: bool,
    pub wake_up_verified: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct RecoveryRevisionFile {
    #[serde(default)]
    verified: bool,
    #[serde(default)]
    recovery_revision: Option<String>,
}

pub struct WarmBootstrapStateStore {
    path: PathBuf,
}

impl WarmBootstrapStateStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn default_path() -> PathBuf {
        codeterm_paths::data_dir()
            .join("mesh")
            .join(WARM_BOOTSTRAP_STATE_FILE)
    }

    pub fn in_data_dir(data_dir: &Path) -> Self {
        Self::new(data_dir.join("mesh").join(WARM_BOOTSTRAP_STATE_FILE))
    }

    fn state_file(&self) -> StateFile {
        StateFile::new(&self.path).with_validator(|text| {
            serde_json::from_str::<WarmBootstrapState>(text)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
    }

    pub fn load(&self) -> Option<WarmBootstrapState> {
        match self.state_file().load(|text| {
            serde_json::from_str::<WarmBootstrapState>(text).map_err(|error| error.to_string())
        }) {
            Loaded::Parsed(state) | Loaded::Recovered { value: state, .. } => Some(state),
            Loaded::Missing | Loaded::Unrecoverable { .. } => None,
        }
    }

    pub fn save(&self, state: &WarmBootstrapState) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let encoded = serde_json::to_string_pretty(state).map_err(|error| error.to_string())?;
        self.state_file().save(&encoded)
    }
}

/// Reads an existing verified recovery revision marker written by restore/recreate flows.
pub fn read_verified_recovery_revision(data_dir: &Path) -> Option<String> {
    let path = data_dir.join("mesh").join(RECOVERY_STATE_FILE);
    let raw = std::fs::read_to_string(path).ok()?;
    let parsed: RecoveryRevisionFile = serde_json::from_str(&raw).ok()?;
    if parsed.verified {
        parsed
            .recovery_revision
            .map(|r| r.trim().to_owned())
            .filter(|r| !r.is_empty())
    } else {
        None
    }
}

pub struct TargetProvenance<'a> {
    pub enrolled_account_id: &'a str,
    pub target_installation_id: &'a str,
    pub authenticated_caller_installation_id: &'a str,
    pub verified_recovery_revision: Option<&'a str>,
}

pub struct WarmBootstrapApplyEnv<'a> {
    pub mem: &'a MemManager,
    pub general_dir: &'a Path,
    pub state_store: &'a WarmBootstrapStateStore,
    pub now_ms: u64,
    pub general_tab_id: Option<String>,
    pub providers: Vec<ProviderSignInCardStatus>,
}

/// Returns true only when the target has no prior non-bundled owner memory or local identity file.
pub fn is_target_mem_empty(mem: &MemManager, general_dir: &Path) -> Result<bool, String> {
    if let Ok(identity_file) = std::fs::read_to_string(general_dir.join("identity.md")) {
        if !identity_file.trim().is_empty() {
            return Ok(false);
        }
    }
    let graph = mem.graph_full()?;
    for node in graph.nodes {
        if node.tags.iter().any(|t| t == "bundled") {
            continue;
        }
        if let Ok(Some(item)) = mem.get(&node.id) {
            let bundled = item
                .source
                .as_deref()
                .is_some_and(|s| s.starts_with("bundled:"));
            if !bundled {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn rejected_result(
    generation: &str,
    reason: WarmBootstrapRejectReason,
    warning: impl Into<String>,
    seed_started_at_ms: u64,
) -> WarmBootstrapApplyResult {
    WarmBootstrapApplyResult {
        schema_version: WARM_BOOTSTRAP_SCHEMA_VERSION,
        status: WarmBootstrapStatus::Rejected,
        reject_reason: Some(reason),
        recovery_revision: None,
        creation_generation: generation.to_owned(),
        applied_counts: WarmBootstrapAppliedCounts::default(),
        excluded: Vec::new(),
        warnings: vec![warning.into()],
        readable_owner_context: false,
        wake_up_verified: false,
        general_tab_id: None,
        providers: Vec::new(),
        seed_started_at_ms: Some(seed_started_at_ms),
        warm_console_ready_at_ms: None,
        warm_bootstrap_delta_secs: None,
        within_budget: None,
    }
}

/// Applies allowlisted portable preferences onto a target AppConfig without ever touching
/// target-owned authority fields (account, billing, api, neighbors, mesh identity, secrets).
pub fn apply_portable_preferences_to_config(
    config: &mut AppConfig,
    prefs: &PortablePreferences,
) -> usize {
    let mut fields = 0usize;
    if let Some(provider) = prefs.general_agent.agent_provider.as_deref() {
        config.general_agent.agent_provider = provider.to_owned();
        fields += 1;
    }
    if let Some(model) = prefs.general_agent.model.as_deref() {
        config.general_agent.model = Some(model.to_owned());
        fields += 1;
    }
    if let Some(effort) = prefs.general_agent.reasoning_effort.as_deref() {
        config.general_agent.reasoning_effort = Some(effort.to_owned());
        fields += 1;
    }
    if let Some(hints) = prefs.general_agent.capability_hints {
        config.general_agent.capability_hints = hints;
        fields += 1;
    }
    if let Some(budget) = prefs.memory.l1_budget_tokens {
        config.memory.l1_budget_tokens = budget;
        fields += 1;
    }
    if let Some(auto_extract) = prefs.memory.auto_extract {
        config.memory.auto_extract = auto_extract;
        fields += 1;
    }
    if let Some(librarian) = prefs.memory.librarian_enabled {
        config.memory.librarian_enabled = librarian;
        fields += 1;
    }
    if let Some(idle_mins) = prefs.network.cloud_idle_sleep_minutes {
        config.mesh.cloud_idle_sleep_minutes = idle_mins;
        fields += 1;
    }
    if let Some(max_awake) = prefs.network.cloud_max_awake_without_owner_minutes {
        config.mesh.cloud_max_awake_without_owner_minutes = max_awake;
        fields += 1;
    }
    if let Some(resume) = prefs.agent.resume_sessions {
        config.agent.resume_sessions = resume;
        fields += 1;
    }
    if let Some(cont) = prefs.agent.continue_after_restart {
        config.agent.continue_after_restart = cont;
        fields += 1;
    }
    if let Some(ceiling) = prefs.agent.sandbox_ceiling.as_deref() {
        config.agent.sandbox_ceiling = Some(ceiling.to_owned());
        fields += 1;
    }
    for provider in &prefs.configured_providers {
        let entry = config
            .agents
            .entry(provider.provider_id.clone())
            .or_default();
        if let Some(model) = &provider.default_model {
            entry.default_model = Some(model.clone());
            fields += 1;
        }
        if let Some(resume) = provider.resume_sessions {
            entry.resume_sessions = Some(resume);
            fields += 1;
        }
    }
    fields
}

/// Applies a bounded, versioned owner projection onto a fresh cloud machine target.
///
/// Enforces:
/// - Schema version and projection byte cap
/// - Verified recovery revision preservation (never seeds over restored owner state)
/// - Same-account, target-installation, and owner-installation provenance
/// - Idempotent retry for matching creation generation (preserves local edits)
/// - Rejection of stale/different creation generations and non-empty targets
/// - Strict secret/path filtering and read-back verification before reporting ready
pub fn apply_warm_bootstrap<F, G>(
    projection: WarmBootstrapProjection,
    provenance: &TargetProvenance<'_>,
    env: &WarmBootstrapApplyEnv<'_>,
    mut apply_prefs: F,
    mut on_console_ready: G,
) -> WarmBootstrapApplyResult
where
    F: FnMut(&PortablePreferences) -> Result<usize, String>,
    G: FnMut() -> Result<(), String>,
{
    let seed_started_at_ms = env.now_ms;
    let generation = projection.creation_generation.trim().to_owned();

    if projection.schema_version != WARM_BOOTSTRAP_SCHEMA_VERSION {
        return rejected_result(
            &generation,
            WarmBootstrapRejectReason::UnsupportedSchema,
            format!(
                "Unsupported warm bootstrap schema version {}",
                projection.schema_version
            ),
            seed_started_at_ms,
        );
    }

    if serde_json::to_vec(&projection)
        .map(|b| b.len())
        .unwrap_or(usize::MAX)
        > MAX_PROJECTION_BYTES
    {
        return rejected_result(
            &generation,
            WarmBootstrapRejectReason::OversizedProjection,
            "Warm bootstrap projection exceeds maximum allowed size",
            seed_started_at_ms,
        );
    }

    // Verified recovery revision always wins: never overwrite restored owner state.
    let existing = env.state_store.load();
    let active_recovery_revision = provenance
        .verified_recovery_revision
        .map(str::to_owned)
        .or_else(|| existing.as_ref().and_then(|s| s.recovery_revision.clone()))
        .or_else(|| {
            projection
                .recovery_revision
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        });
    if let Some(rev) = active_recovery_revision {
        let wake_up_ok = env
            .mem
            .wake_up(&WakeUpOptions::default())
            .is_ok_and(|text| !text.trim().is_empty());
        return WarmBootstrapApplyResult {
            schema_version: WARM_BOOTSTRAP_SCHEMA_VERSION,
            status: WarmBootstrapStatus::SkippedRestoredState,
            reject_reason: None,
            recovery_revision: Some(rev),
            creation_generation: generation,
            applied_counts: WarmBootstrapAppliedCounts::default(),
            excluded: Vec::new(),
            warnings: vec![
                "Verified recovery revision present; preserved restored owner state without fresh seeding".into(),
            ],
            readable_owner_context: wake_up_ok,
            wake_up_verified: wake_up_ok,
            general_tab_id: env.general_tab_id.clone(),
            providers: env.providers.clone(),
            seed_started_at_ms: Some(seed_started_at_ms),
            warm_console_ready_at_ms: wake_up_ok.then_some(env.now_ms),
            warm_bootstrap_delta_secs: wake_up_ok.then_some(0.0),
            within_budget: wake_up_ok.then_some(true),
        };
    }

    // Provenance verification (same account, current owning installation, target installation, non-empty generation).
    if provenance.enrolled_account_id.trim().is_empty()
        || provenance.target_installation_id.trim().is_empty()
        || provenance
            .authenticated_caller_installation_id
            .trim()
            .is_empty()
        || generation.is_empty()
    {
        return rejected_result(
            &generation,
            WarmBootstrapRejectReason::InvalidProvenance,
            "Missing enrolled account, installation, or creation generation provenance",
            seed_started_at_ms,
        );
    }
    if projection.account_id.trim() != provenance.enrolled_account_id.trim() {
        return rejected_result(
            &generation,
            WarmBootstrapRejectReason::WrongAccount,
            "Warm bootstrap projection account does not match enrolled target account",
            seed_started_at_ms,
        );
    }
    if projection.target_installation_id.trim() != provenance.target_installation_id.trim()
        || projection.owner_installation_id.trim()
            != provenance.authenticated_caller_installation_id.trim()
    {
        return rejected_result(
            &generation,
            WarmBootstrapRejectReason::InstallationMismatch,
            "Warm bootstrap projection installation provenance does not match authenticated peer or target",
            seed_started_at_ms,
        );
    }

    // Idempotent retry vs stale generation check.
    if let Some(prev) = existing {
        if prev.creation_generation == generation
            && prev.account_id == provenance.enrolled_account_id
            && prev.target_installation_id == provenance.target_installation_id
        {
            let wake_up_ok = env
                .mem
                .wake_up(&WakeUpOptions::default())
                .is_ok_and(|text| !text.trim().is_empty());
            return WarmBootstrapApplyResult {
                schema_version: WARM_BOOTSTRAP_SCHEMA_VERSION,
                status: WarmBootstrapStatus::AlreadyApplied,
                reject_reason: None,
                recovery_revision: prev.recovery_revision,
                creation_generation: generation,
                applied_counts: WarmBootstrapAppliedCounts::default(),
                excluded: Vec::new(),
                warnings: Vec::new(),
                readable_owner_context: wake_up_ok && prev.readable_owner_context,
                wake_up_verified: wake_up_ok && prev.wake_up_verified,
                general_tab_id: env.general_tab_id.clone(),
                providers: env.providers.clone(),
                seed_started_at_ms: Some(seed_started_at_ms),
                warm_console_ready_at_ms: (wake_up_ok && prev.readable_owner_context)
                    .then_some(env.now_ms),
                warm_bootstrap_delta_secs: (wake_up_ok && prev.readable_owner_context)
                    .then_some(0.0),
                within_budget: (wake_up_ok && prev.readable_owner_context).then_some(true),
            };
        }
        return rejected_result(
            &generation,
            WarmBootstrapRejectReason::StaleGeneration,
            "Warm bootstrap rejected: target already committed a different creation generation",
            seed_started_at_ms,
        );
    }

    // Fresh-create truly-empty guard: never overwrite pre-existing local owner state.
    match is_target_mem_empty(env.mem, env.general_dir) {
        Ok(true) => {}
        Ok(false) => {
            return rejected_result(
                &generation,
                WarmBootstrapRejectReason::TargetNotEmpty,
                "Warm bootstrap refused: target already contains local owner state",
                seed_started_at_ms,
            );
        }
        Err(error) => {
            return rejected_result(
                &generation,
                WarmBootstrapRejectReason::ReadbackUnverified,
                format!("Could not verify fresh target state: {error}"),
                seed_started_at_ms,
            );
        }
    }

    // Defense-in-depth sanitization on the target.
    let (clean, excluded) = sanitize_projection(projection);
    let digest = projection_digest(&clean);
    let mut counts = WarmBootstrapAppliedCounts::default();
    let mut warnings = Vec::new();
    let mut expected_canary: Option<String> = None;

    if let Err(error) = std::fs::create_dir_all(env.general_dir) {
        warnings.push(format!("Could not prepare general dir: {error}"));
    }

    crate::mem::store::with_author(
        crate::mem::store::MemAuthor::owner(Some(clean.account_id.clone())),
        || {
            // Apply identity (L0)
            if let Some(ref identity) = clean.identity {
                if let Some(ref id_md) = identity.identity_md {
                    if let Err(error) = std::fs::write(env.general_dir.join("identity.md"), id_md) {
                        warnings.push(format!("identity.md write failed: {error}"));
                    }
                    let item = NewMemItem {
                        kind: MemKind::Preference,
                        scope: MemScope::Global,
                        title: "identity".into(),
                        summary: Some(
                            "User identity — name, role, location, communication style".into(),
                        ),
                        area: None,
                        tags: vec!["identity".into(), "warm-seed".into()],
                        body: id_md.clone(),
                        source: Some("warm-seed:home".into()),
                        importance: 5.0,
                        source_path: None,
                        pinned: false,
                    };
                    match env.mem.save_replace(item) {
                        Ok(_) => {
                            counts.identity_fields += 1;
                            if expected_canary.is_none() {
                                expected_canary = id_md
                                    .lines()
                                    .next()
                                    .map(|l| l.trim().chars().take(48).collect());
                            }
                        }
                        Err(error) => warnings.push(format!("identity mem save failed: {error}")),
                    }
                }
                if let Some(ref soul_md) = identity.soul_md {
                    if let Err(error) = std::fs::write(env.general_dir.join("soul.md"), soul_md) {
                        warnings.push(format!("soul.md write failed: {error}"));
                    } else {
                        counts.identity_fields += 1;
                    }
                }
                if let Some(ref nick) = identity.nickname {
                    let item = NewMemItem {
                        kind: MemKind::Preference,
                        scope: MemScope::Global,
                        title: "nickname".into(),
                        summary: Some("Preferred owner nickname".into()),
                        area: None,
                        tags: vec!["identity".into(), "warm-seed".into()],
                        body: nick.clone(),
                        source: Some("warm-seed:home".into()),
                        importance: 5.0,
                        source_path: None,
                        pinned: false,
                    };
                    match env.mem.save_replace(item) {
                        Ok(_) => counts.identity_fields += 1,
                        Err(error) => warnings.push(format!("nickname mem save failed: {error}")),
                    }
                }
            }

            // Apply L1 wake-up facts and approved owner rules
            for entry in &clean.wake_up_items {
                let mut tags = entry.tags.clone();
                if !tags.iter().any(|t| t == "warm-seed") {
                    tags.push("warm-seed".into());
                }
                let item = NewMemItem {
                    kind: entry.kind.to_mem_kind(),
                    scope: MemScope::Global,
                    title: entry.title.clone(),
                    summary: entry.summary.clone(),
                    area: entry.area.clone(),
                    tags,
                    body: entry.body.clone(),
                    source: Some("warm-seed:home".into()),
                    importance: entry.importance,
                    source_path: None,
                    pinned: entry.pinned,
                };
                match env.mem.save_replace(item) {
                    Ok(_) => {
                        counts.wake_up_items += 1;
                        if matches!(entry.kind, super::types::PortableMemKind::Rule) {
                            counts.rules += 1;
                        }
                        if expected_canary.is_none() {
                            expected_canary = Some(entry.title.clone());
                        }
                    }
                    Err(error) => {
                        warnings.push(format!("wake-up item '{}' failed: {error}", entry.title))
                    }
                }
            }

            // Apply referenced registered skill versions
            for skill in &clean.skills {
                let mut tags = skill.tags.clone();
                let ver_tag = format!("version:{}", skill.version);
                if !tags.iter().any(|t| t == &ver_tag) {
                    tags.push(ver_tag);
                }
                if !tags.iter().any(|t| t == "warm-seed") {
                    tags.push("warm-seed".into());
                }
                let item = NewMemItem {
                    kind: MemKind::Skill,
                    scope: MemScope::Global,
                    title: skill.title.clone(),
                    summary: skill.summary.clone(),
                    area: skill.area.clone(),
                    tags,
                    body: skill.body.clone(),
                    source: Some(format!("warm-seed:skill:{}@{}", skill.slug, skill.version)),
                    importance: skill.importance,
                    source_path: None,
                    pinned: skill.pinned,
                };
                match env.mem.save_replace(item) {
                    Ok(_) => counts.skills += 1,
                    Err(error) => warnings.push(format!("skill '{}' failed: {error}", skill.slug)),
                }
            }
        },
    );

    // Apply allowlisted non-secret Domios & network preferences
    match apply_prefs(&clean.preferences) {
        Ok(applied) => counts.preference_fields = applied,
        Err(error) => warnings.push(format!("portable preferences apply failed: {error}")),
    }

    // Regenerate first-console instructions before read-back check
    if let Err(error) = on_console_ready() {
        warnings.push(format!("first-console instruction refresh failed: {error}"));
    }

    // Mandatory read-back verification (RED LINE: never show ready without readable owner-context result)
    let wake_up_text = env.mem.wake_up(&WakeUpOptions::default());
    let (readable_owner_context, wake_up_verified) = match wake_up_text {
        Ok(text) => {
            let trimmed = text.trim();
            let non_empty = !trimmed.is_empty();
            let contains_expected = expected_canary
                .as_deref()
                .map(|needle| !needle.is_empty() && trimmed.contains(needle))
                .unwrap_or(non_empty);
            (
                non_empty && contains_expected,
                non_empty && contains_expected,
            )
        }
        Err(error) => {
            warnings.push(format!("wake-up read-back failed: {error}"));
            (false, false)
        }
    };

    let status = if !readable_owner_context {
        WarmBootstrapStatus::Failed
    } else if !excluded.is_empty() || !warnings.is_empty() {
        WarmBootstrapStatus::Partial
    } else {
        WarmBootstrapStatus::Applied
    };

    let completed_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(env.now_ms)
        .max(seed_started_at_ms);

    let persist_state = WarmBootstrapState {
        schema_version: WARM_BOOTSTRAP_SCHEMA_VERSION,
        target_installation_id: clean.target_installation_id.clone(),
        account_id: clean.account_id.clone(),
        owner_installation_id: clean.owner_installation_id.clone(),
        creation_generation: clean.creation_generation.clone(),
        applied_at_ms: completed_at_ms,
        outcome: status,
        recovery_revision: None,
        projection_digest: digest,
        readable_owner_context,
        wake_up_verified,
    };
    if readable_owner_context {
        if let Err(error) = env.state_store.save(&persist_state) {
            warnings.push(format!("persist warm bootstrap receipt failed: {error}"));
        }
    }

    let delta_secs = readable_owner_context
        .then(|| (completed_at_ms.saturating_sub(seed_started_at_ms) as f64) / 1000.0);

    WarmBootstrapApplyResult {
        schema_version: WARM_BOOTSTRAP_SCHEMA_VERSION,
        status,
        reject_reason: (!readable_owner_context)
            .then_some(WarmBootstrapRejectReason::ReadbackUnverified),
        recovery_revision: None,
        creation_generation: clean.creation_generation,
        applied_counts: counts,
        excluded,
        warnings,
        readable_owner_context,
        wake_up_verified,
        general_tab_id: env.general_tab_id.clone(),
        providers: env.providers.clone(),
        seed_started_at_ms: Some(seed_started_at_ms),
        warm_console_ready_at_ms: readable_owner_context.then_some(completed_at_ms),
        warm_bootstrap_delta_secs: delta_secs,
        within_budget: delta_secs.map(|d| d <= WARM_BOOTSTRAP_BUDGET_SECS),
    }
}
