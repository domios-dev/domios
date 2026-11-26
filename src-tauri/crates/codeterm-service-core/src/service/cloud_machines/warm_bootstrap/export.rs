use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use codeterm_plugin::agent_runtime::RuntimeLaunchProfile;
use sha2::{Digest, Sha256};

use crate::config::schema::AppConfig;
use crate::mem::MemManager;

use super::filter::{sanitize_projection, valid_portable_slug};
use super::types::{
    ExclusionCategory, ExclusionReason, PortableAcpProfileRef, PortableAgentPrefs,
    PortableGeneralAgentPrefs, PortableMemEntry, PortableMemKind, PortableMemoryPrefs,
    PortableNetworkPrefs, PortableOwnerIdentity, PortablePreferences, PortableProviderRef,
    PortableRegisteredSkill, PortableSkillSource, SanitizedExclusion, WarmBootstrapProjection,
    MAX_REGISTERED_SKILLS, MAX_WAKE_UP_ITEMS, WARM_BOOTSTRAP_SCHEMA_VERSION,
};

pub struct HomeExportInput<'a> {
    pub account_id: &'a str,
    pub owner_installation_id: &'a str,
    pub target_installation_id: &'a str,
    pub creation_generation: &'a str,
    pub recovery_revision: Option<&'a str>,
    pub mem: &'a MemManager,
    pub general_dir: &'a Path,
    pub config: &'a AppConfig,
    pub acp_owner_profiles: &'a BTreeMap<String, RuntimeLaunchProfile>,
    pub available_provider_ids: &'a [String],
}

fn is_bundled_source(source: Option<&str>) -> bool {
    source.is_some_and(|s| s.starts_with("bundled:"))
}

fn slug_from_title(title: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in title.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            slug.push(lower);
            prev_dash = false;
        } else if (lower == '-' || lower == '_' || lower.is_whitespace())
            && !slug.is_empty()
            && !prev_dash
        {
            slug.push('-');
            prev_dash = true;
        }
    }
    let trimmed = slug.trim_matches('-').to_owned();
    trimmed.chars().take(64).collect()
}

fn extract_skill_version(item: &crate::mem::MemItem) -> String {
    for tag in &item.tags {
        if let Some(version) = tag.strip_prefix("version:") {
            if !version.trim().is_empty() {
                return version.trim().to_owned();
            }
        }
    }
    let digest = Sha256::digest(item.body.as_bytes());
    let hex = format!("{digest:x}");
    format!("sha256:{}", &hex[..16])
}

fn classify_skill_source(item: &crate::mem::MemItem) -> Option<PortableSkillSource> {
    let source = item.source.as_deref().unwrap_or("");
    if source.starts_with("bundled:") {
        return None;
    }
    if let Some(plugin_id) =
        source.strip_prefix(codeterm_plugin::agent_verbs::PLUGIN_SKILL_SOURCE_PREFIX)
    {
        let id = plugin_id.split(':').next().unwrap_or("").trim();
        if !id.is_empty() {
            return Some(PortableSkillSource::PluginSkillRef {
                plugin_id: id.to_owned(),
                version: extract_skill_version(item),
            });
        }
        return None;
    }
    if source.starts_with("file:") {
        // Only skills inside Domios's own skills directory are trusted folder skills.
        let lower = source.to_ascii_lowercase();
        if lower.contains("/skills/") || lower.contains("\\skills\\") {
            return Some(PortableSkillSource::TrustedFolderSkill);
        }
        return None;
    }
    Some(PortableSkillSource::MemSkill)
}

/// Builds a strictly bounded, versioned, explicitly portable owner projection from home state.
/// Never reads or exports secrets, vault entries, provider homes, auth files, or raw configs.
pub fn build_owner_projection(
    input: &HomeExportInput<'_>,
) -> (WarmBootstrapProjection, Vec<SanitizedExclusion>) {
    let mut pre_exclusions = Vec::new();

    // 1. Identity (L0)
    let identity_md = std::fs::read_to_string(input.general_dir.join("identity.md"))
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            input
                .mem
                .get("preference_identity")
                .ok()
                .flatten()
                .filter(|item| !is_bundled_source(item.source.as_deref()))
                .filter(|item| !item.author.is_neighbor())
                .map(|item| item.body.trim().to_owned())
                .filter(|s| !s.is_empty())
        });
    let soul_md = std::fs::read_to_string(input.general_dir.join("soul.md"))
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    let nickname = input
        .mem
        .get("preference_nickname")
        .ok()
        .flatten()
        .filter(|item| !is_bundled_source(item.source.as_deref()))
        .filter(|item| !item.author.is_neighbor())
        .map(|item| item.body.trim().to_owned())
        .filter(|s| !s.is_empty());

    let identity = if identity_md.is_some() || soul_md.is_some() || nickname.is_some() {
        Some(PortableOwnerIdentity {
            identity_md,
            soul_md,
            nickname,
        })
    } else {
        None
    };

    // 2. L1 wake-up facts, approved owner rules, preferences & registered skills
    let pinned_items = input.mem.list_pinned(64).unwrap_or_default();
    let pinned_ids: HashSet<String> = pinned_items.iter().map(|item| item.id.clone()).collect();

    let mut wake_up_candidates: Vec<(bool, crate::mem::MemItem)> = Vec::new();
    let mut skill_candidates: Vec<(bool, crate::mem::MemItem)> = Vec::new();
    let mut seen_ids = HashSet::new();

    for item in pinned_items {
        if !seen_ids.insert(item.id.clone()) {
            continue;
        }
        if item.id == "preference_identity" || item.id == "preference_nickname" {
            continue;
        }
        if item.author.is_neighbor() {
            pre_exclusions.push(SanitizedExclusion {
                category: ExclusionCategory::WakeUpItem,
                reason: ExclusionReason::NeighborAuthored,
            });
            continue;
        }
        if is_bundled_source(item.source.as_deref()) || item.scope != "global" {
            continue;
        }
        if item.kind == "skill" {
            skill_candidates.push((true, item));
        } else if PortableMemKind::from_mem_kind_str(&item.kind).is_some() {
            wake_up_candidates.push((true, item));
        }
    }

    if let Ok(graph) = input.mem.graph_full() {
        let mut nodes = graph.nodes;
        nodes.sort_by(|a, b| {
            b.importance
                .partial_cmp(&a.importance)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        for node in nodes {
            if !seen_ids.insert(node.id.clone()) {
                continue;
            }
            if node.id == "preference_identity" || node.id == "preference_nickname" {
                continue;
            }
            if node
                .author
                .as_ref()
                .is_some_and(crate::mem::store::MemAuthor::is_neighbor)
            {
                pre_exclusions.push(SanitizedExclusion {
                    category: ExclusionCategory::WakeUpItem,
                    reason: ExclusionReason::NeighborAuthored,
                });
                continue;
            }
            if node
                .tags
                .iter()
                .any(|t| t == "auto-extracted" || t == "bundled")
            {
                continue;
            }
            let wants_skill =
                node.kind == "skill" && skill_candidates.len() < MAX_REGISTERED_SKILLS * 2;
            let wants_wakeup = PortableMemKind::from_mem_kind_str(&node.kind).is_some()
                && wake_up_candidates.len() < MAX_WAKE_UP_ITEMS * 2;
            if !wants_skill && !wants_wakeup {
                continue;
            }
            let Ok(Some(item)) = input.mem.get(&node.id) else {
                continue;
            };
            if item.author.is_neighbor()
                || is_bundled_source(item.source.as_deref())
                || item.scope != "global"
            {
                continue;
            }
            let is_pinned = pinned_ids.contains(&item.id);
            if item.kind == "skill" {
                skill_candidates.push((is_pinned, item));
            } else {
                wake_up_candidates.push((is_pinned, item));
            }
        }
    }

    let wake_up_items: Vec<PortableMemEntry> = wake_up_candidates
        .into_iter()
        .filter_map(|(pinned, item)| {
            let kind = PortableMemKind::from_mem_kind_str(&item.kind)?;
            Some(PortableMemEntry {
                kind,
                title: item.title,
                summary: item.summary,
                area: item.area,
                tags: item.tags,
                body: item.body,
                importance: item.importance,
                pinned,
            })
        })
        .collect();

    let mut skills = Vec::new();
    for (pinned, item) in skill_candidates {
        let Some(registry_source) = classify_skill_source(&item) else {
            pre_exclusions.push(SanitizedExclusion {
                category: ExclusionCategory::Skill,
                reason: ExclusionReason::UntrustedSkillSource,
            });
            continue;
        };
        let slug = slug_from_title(&item.title);
        if !valid_portable_slug(&slug) {
            pre_exclusions.push(SanitizedExclusion {
                category: ExclusionCategory::Skill,
                reason: ExclusionReason::InvalidSlug,
            });
            continue;
        }
        let version = extract_skill_version(&item);
        skills.push(PortableRegisteredSkill {
            slug,
            title: item.title,
            version,
            registry_source,
            summary: item.summary,
            area: item.area,
            tags: item.tags,
            body: item.body,
            importance: item.importance,
            pinned,
        });
    }

    // 3. Strictly allowlisted non-secret Domios & network preferences
    let mut configured_provider_ids: Vec<String> = Vec::new();
    let mut push_provider = |id: &str| {
        let trimmed = id.trim();
        if !trimmed.is_empty()
            && !configured_provider_ids
                .iter()
                .any(|existing| existing == trimmed)
        {
            configured_provider_ids.push(trimmed.to_owned());
        }
    };
    push_provider(&input.config.general_agent.agent_provider);
    let mut sorted_agent_keys: Vec<&String> = input.config.agents.keys().collect();
    sorted_agent_keys.sort();
    for id in sorted_agent_keys {
        push_provider(id);
    }
    for id in input.available_provider_ids {
        push_provider(id);
    }

    let configured_providers: Vec<PortableProviderRef> = configured_provider_ids
        .into_iter()
        .map(|provider_id| {
            let cfg = input.config.agents.get(&provider_id);
            PortableProviderRef {
                provider_id,
                default_model: cfg.and_then(|c| c.default_model.clone()),
                resume_sessions: cfg.and_then(|c| c.resume_sessions),
            }
        })
        .collect();

    // Only export non-secret ACP profile identity references — never env/secret_env/auth_env values.
    let acp_profiles: Vec<PortableAcpProfileRef> = input
        .acp_owner_profiles
        .iter()
        .map(|(profile_id, profile)| PortableAcpProfileRef {
            profile_id: profile_id.clone(),
            registry_id: profile.registry_id.clone(),
        })
        .collect();

    let preferences = PortablePreferences {
        general_agent: PortableGeneralAgentPrefs {
            agent_provider: Some(input.config.general_agent.agent_provider.clone()),
            model: input.config.general_agent.model.clone(),
            reasoning_effort: input.config.general_agent.reasoning_effort.clone(),
            capability_hints: Some(input.config.general_agent.capability_hints),
        },
        memory: PortableMemoryPrefs {
            l1_budget_tokens: Some(input.config.memory.l1_budget_tokens),
            auto_extract: Some(input.config.memory.auto_extract),
            librarian_enabled: Some(input.config.memory.librarian_enabled),
        },
        network: PortableNetworkPrefs {
            cloud_idle_sleep_minutes: Some(input.config.mesh.cloud_idle_sleep_minutes),
            cloud_max_awake_without_owner_minutes: Some(
                input.config.mesh.cloud_max_awake_without_owner_minutes,
            ),
        },
        agent: PortableAgentPrefs {
            resume_sessions: Some(input.config.agent.resume_sessions),
            continue_after_restart: Some(input.config.agent.continue_after_restart),
            sandbox_ceiling: input.config.agent.sandbox_ceiling.clone(),
        },
        configured_providers,
        acp_profiles,
    };

    let raw = WarmBootstrapProjection {
        schema_version: WARM_BOOTSTRAP_SCHEMA_VERSION,
        account_id: input.account_id.to_owned(),
        owner_installation_id: input.owner_installation_id.to_owned(),
        target_installation_id: input.target_installation_id.to_owned(),
        creation_generation: input.creation_generation.to_owned(),
        recovery_revision: input.recovery_revision.map(str::to_owned),
        identity,
        wake_up_items,
        skills,
        preferences,
    };
    let (clean, mut filter_exclusions) = sanitize_projection(raw);
    pre_exclusions.append(&mut filter_exclusions);
    (clean, pre_exclusions)
}
