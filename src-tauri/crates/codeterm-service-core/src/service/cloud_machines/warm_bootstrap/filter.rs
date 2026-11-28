use sha2::{Digest, Sha256};

use super::types::{
    ExclusionCategory, ExclusionReason, PortableAcpProfileRef, PortableAgentPrefs,
    PortableGeneralAgentPrefs, PortableMemEntry, PortableMemoryPrefs, PortableNetworkPrefs,
    PortableOwnerIdentity, PortablePreferences, PortableProviderRef, PortableRegisteredSkill,
    PortableSkillSource, SanitizedExclusion, WarmBootstrapProjection, MAX_IDENTITY_BYTES,
    MAX_ITEM_BODY_BYTES, MAX_REGISTERED_SKILLS, MAX_SKILL_BODY_BYTES, MAX_WAKE_UP_ITEMS,
};

const SECRET_PREFIXES: &[&str] = &[
    "sk-ant-",
    "sk-proj-",
    "sk-live-",
    "sk-test-",
    "sk-",
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "github_pat_",
    concat!("AI", "za"),
    "xoxb-",
    "xoxp-",
    "xoxa-",
    "AKIA",
    "ASIA",
    "ya29.",
    "hf_",
    "glpat-",
    "npm_",
    "Bearer ",
    "-----BEGIN ",
];

const SECRET_ASSIGNMENT_KEYS: &[&str] = &[
    "API_KEY=",
    "SECRET_KEY=",
    "ACCESS_TOKEN=",
    "REFRESH_TOKEN=",
    "PRIVATE_KEY=",
    "CLIENT_SECRET=",
    "PASSWORD=",
    "PASSWD=",
    "CODETERM_API_TOKEN=",
    "DOMIOS_BOOTSTRAP_",
    "ANTHROPIC_API_KEY=",
    "OPENAI_API_KEY=",
    "GEMINI_API_KEY=",
    "GOOGLE_API_KEY=",
    "FLY_API_TOKEN=",
];

const FORBIDDEN_PATH_TOKENS: &[&str] = &[
    "~/.config/",
    "/.config/gcloud",
    "/.domios/secrets",
    "\\.domios\\secrets",
    "/.codeterm/secrets",
    "\\.codeterm\\secrets",
    "auth.json",
    "credentials.json",
    "id_rsa",
    "id_ed25519",
    ".netrc",
    ".npmrc",
    ".env",
    "vault://",
];

const RESERVED_WIN_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Detects PEM blocks, known API/OAuth token prefixes, JWTs, and secret environment assignments.
pub fn contains_secret_shaped_content(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    let upper = text.to_ascii_uppercase();
    if upper.contains("PRIVATE KEY")
        || SECRET_ASSIGNMENT_KEYS
            .iter()
            .any(|needle| upper.contains(needle))
    {
        return true;
    }
    if SECRET_PREFIXES.iter().any(|prefix| {
        if *prefix == "sk-" {
            has_token_with_prefix(text, "sk-", 20)
        } else if *prefix == "Bearer " {
            has_bearer_token(text)
        } else {
            text.contains(prefix)
        }
    }) {
        return true;
    }
    has_jwt_like_token(text)
}

fn has_token_with_prefix(text: &str, prefix: &str, min_len: usize) -> bool {
    text.split(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ',' | ';' | '(' | ')' | '`'))
        .any(|token| {
            token.starts_with(prefix)
                && token.len() >= min_len
                && token[prefix.len()..]
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        })
}

fn has_bearer_token(text: &str) -> bool {
    let mut rest = text;
    while let Some(idx) = rest.find("Bearer ") {
        let after = &rest[idx + "Bearer ".len()..];
        let token = after
            .split(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ',' | ';'))
            .next()
            .unwrap_or("");
        if token.len() >= 12
            && token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-.~+/=".contains(&b))
        {
            return true;
        }
        rest = after;
    }
    false
}

fn has_jwt_like_token(text: &str) -> bool {
    text.split(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ',' | ';' | '(' | ')' | '`'))
        .any(|token| {
            if !token.starts_with("eyJ") {
                return false;
            }
            let parts: Vec<&str> = token.split('.').collect();
            parts.len() == 3
                && parts.iter().all(|part| {
                    part.len() >= 8
                        && part
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                })
        })
}

/// Detects hidden dot-homes (provider/tool credential dirs), vault/secret file paths, and non-portable host home paths.
pub fn contains_forbidden_path(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if FORBIDDEN_PATH_TOKENS
        .iter()
        .any(|token| lower.contains(token))
    {
        return true;
    }
    has_hidden_home_path(&lower) || has_non_portable_host_path(text)
}

fn has_hidden_home_path(lower: &str) -> bool {
    lower.contains("~/.") || lower.contains("~\\.")
}

fn has_non_portable_host_path(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if lower.contains("/users/")
        || lower.contains("/home/")
        || lower.contains("c:\\users\\")
        || lower.contains("c:/users/")
        || lower.contains("\\\\?\\")
    {
        return true;
    }
    false
}

pub fn valid_portable_slug(slug: &str) -> bool {
    if slug.is_empty() || slug.len() > 64 {
        return false;
    }
    if slug.contains("..") || slug.contains('/') || slug.contains('\\') {
        return false;
    }
    let upper = slug.to_ascii_uppercase();
    if RESERVED_WIN_NAMES.contains(&upper.as_str()) {
        return false;
    }
    let bytes = slug.as_bytes();
    bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-' || *b == b'_')
}

pub fn valid_sandbox_ceiling(value: &str) -> bool {
    matches!(value, "read-only" | "workspace-write")
}

pub fn valid_identifier(value: &str, max_len: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_len
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.@/:".contains(&b))
}

fn valid_provider_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}

fn valid_skill_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 96
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._+-:".contains(&b))
}

fn valid_skill_source(source: &PortableSkillSource) -> bool {
    match source {
        PortableSkillSource::MemSkill | PortableSkillSource::TrustedFolderSkill => true,
        PortableSkillSource::PluginSkillRef { plugin_id, version } => {
            valid_provider_id(plugin_id) && valid_skill_version(version)
        }
    }
}

fn sanitize_identity(
    identity: Option<PortableOwnerIdentity>,
    excluded: &mut Vec<SanitizedExclusion>,
) -> Option<PortableOwnerIdentity> {
    let raw = identity?;
    let mut clean_field = |val: Option<String>, max_bytes: usize| -> Option<String> {
        let s = val?;
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return None;
        }
        if trimmed.len() > max_bytes {
            excluded.push(SanitizedExclusion {
                category: ExclusionCategory::Identity,
                reason: ExclusionReason::Oversized,
            });
            return None;
        }
        if contains_secret_shaped_content(trimmed) {
            excluded.push(SanitizedExclusion {
                category: ExclusionCategory::Identity,
                reason: ExclusionReason::SecretShapedContent,
            });
            return None;
        }
        if contains_forbidden_path(trimmed) {
            excluded.push(SanitizedExclusion {
                category: ExclusionCategory::Identity,
                reason: ExclusionReason::ForbiddenPath,
            });
            return None;
        }
        Some(trimmed.to_owned())
    };
    let identity_md = clean_field(raw.identity_md, MAX_IDENTITY_BYTES);
    let soul_md = clean_field(raw.soul_md, MAX_IDENTITY_BYTES);
    let nickname = clean_field(raw.nickname, 128);
    if identity_md.is_none() && soul_md.is_none() && nickname.is_none() {
        None
    } else {
        Some(PortableOwnerIdentity {
            identity_md,
            soul_md,
            nickname,
        })
    }
}

fn sanitize_wake_up_items(
    items: Vec<PortableMemEntry>,
    excluded: &mut Vec<SanitizedExclusion>,
) -> Vec<PortableMemEntry> {
    let mut out = Vec::new();
    for item in items {
        if out.len() >= MAX_WAKE_UP_ITEMS {
            excluded.push(SanitizedExclusion {
                category: ExclusionCategory::WakeUpItem,
                reason: ExclusionReason::BoundExceeded,
            });
            continue;
        }
        let title = item.title.trim();
        let body = item.body.trim();
        if title.is_empty() || body.is_empty() || !item.importance.is_finite() {
            excluded.push(SanitizedExclusion {
                category: ExclusionCategory::WakeUpItem,
                reason: ExclusionReason::InvalidValue,
            });
            continue;
        }
        if title.len() > 256 || body.len() > MAX_ITEM_BODY_BYTES {
            excluded.push(SanitizedExclusion {
                category: ExclusionCategory::WakeUpItem,
                reason: ExclusionReason::Oversized,
            });
            continue;
        }
        let summary_bad_secret = item
            .summary
            .as_deref()
            .is_some_and(contains_secret_shaped_content);
        let tags_bad_secret = item.tags.iter().any(|t| contains_secret_shaped_content(t));
        if contains_secret_shaped_content(title)
            || contains_secret_shaped_content(body)
            || summary_bad_secret
            || tags_bad_secret
        {
            excluded.push(SanitizedExclusion {
                category: ExclusionCategory::WakeUpItem,
                reason: ExclusionReason::SecretShapedContent,
            });
            continue;
        }
        if contains_forbidden_path(title)
            || contains_forbidden_path(body)
            || item.summary.as_deref().is_some_and(contains_forbidden_path)
            || item.area.as_deref().is_some_and(contains_forbidden_path)
        {
            excluded.push(SanitizedExclusion {
                category: ExclusionCategory::WakeUpItem,
                reason: ExclusionReason::ForbiddenPath,
            });
            continue;
        }
        out.push(PortableMemEntry {
            kind: item.kind,
            title: title.to_owned(),
            summary: item
                .summary
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty() && s.len() <= 512),
            area: item
                .area
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty() && s.len() <= 128),
            tags: item
                .tags
                .into_iter()
                .map(|t| t.trim().to_owned())
                .filter(|t| !t.is_empty() && t.len() <= 64 && t != "auto-extracted")
                .take(16)
                .collect(),
            body: body.to_owned(),
            importance: item.importance.clamp(1.0, 10.0),
            pinned: item.pinned,
        });
    }
    out
}

fn sanitize_skills(
    skills: Vec<PortableRegisteredSkill>,
    excluded: &mut Vec<SanitizedExclusion>,
) -> Vec<PortableRegisteredSkill> {
    let mut out = Vec::new();
    for skill in skills {
        if out.len() >= MAX_REGISTERED_SKILLS {
            excluded.push(SanitizedExclusion {
                category: ExclusionCategory::Skill,
                reason: ExclusionReason::BoundExceeded,
            });
            continue;
        }
        let slug = skill.slug.trim();
        let title = skill.title.trim();
        let version = skill.version.trim();
        let body = skill.body.trim();
        if !valid_portable_slug(slug) {
            excluded.push(SanitizedExclusion {
                category: ExclusionCategory::Skill,
                reason: ExclusionReason::InvalidSlug,
            });
            continue;
        }
        if !valid_skill_version(version) || !valid_skill_source(&skill.registry_source) {
            excluded.push(SanitizedExclusion {
                category: ExclusionCategory::Skill,
                reason: ExclusionReason::UntrustedSkillSource,
            });
            continue;
        }
        if title.is_empty() || body.is_empty() || !skill.importance.is_finite() {
            excluded.push(SanitizedExclusion {
                category: ExclusionCategory::Skill,
                reason: ExclusionReason::InvalidValue,
            });
            continue;
        }
        if title.len() > 256 || body.len() > MAX_SKILL_BODY_BYTES {
            excluded.push(SanitizedExclusion {
                category: ExclusionCategory::Skill,
                reason: ExclusionReason::Oversized,
            });
            continue;
        }
        if contains_secret_shaped_content(title)
            || contains_secret_shaped_content(body)
            || skill
                .summary
                .as_deref()
                .is_some_and(contains_secret_shaped_content)
        {
            excluded.push(SanitizedExclusion {
                category: ExclusionCategory::Skill,
                reason: ExclusionReason::SecretShapedContent,
            });
            continue;
        }
        if contains_forbidden_path(title)
            || contains_forbidden_path(body)
            || skill
                .summary
                .as_deref()
                .is_some_and(contains_forbidden_path)
        {
            excluded.push(SanitizedExclusion {
                category: ExclusionCategory::Skill,
                reason: ExclusionReason::ForbiddenPath,
            });
            continue;
        }
        out.push(PortableRegisteredSkill {
            slug: slug.to_owned(),
            title: title.to_owned(),
            version: version.to_owned(),
            registry_source: skill.registry_source,
            summary: skill
                .summary
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty() && s.len() <= 512),
            area: skill
                .area
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty() && s.len() <= 128),
            tags: skill
                .tags
                .into_iter()
                .map(|t| t.trim().to_owned())
                .filter(|t| !t.is_empty() && t.len() <= 64)
                .take(16)
                .collect(),
            body: body.to_owned(),
            importance: skill.importance.clamp(1.0, 10.0),
            pinned: skill.pinned,
        });
    }
    out
}

fn sanitize_preferences(
    prefs: PortablePreferences,
    excluded: &mut Vec<SanitizedExclusion>,
) -> PortablePreferences {
    let agent_provider = prefs.general_agent.agent_provider.and_then(|p| {
        let trimmed = p.trim();
        if valid_provider_id(trimmed) {
            Some(trimmed.to_owned())
        } else {
            excluded.push(SanitizedExclusion {
                category: ExclusionCategory::Preference,
                reason: ExclusionReason::InvalidValue,
            });
            None
        }
    });
    let model = prefs.general_agent.model.and_then(|m| {
        let trimmed = m.trim();
        if contains_secret_shaped_content(trimmed) {
            excluded.push(SanitizedExclusion {
                category: ExclusionCategory::Preference,
                reason: ExclusionReason::SecretShapedContent,
            });
            None
        } else if !trimmed.is_empty() && trimmed.len() <= 128 && valid_identifier(trimmed, 128) {
            Some(trimmed.to_owned())
        } else {
            excluded.push(SanitizedExclusion {
                category: ExclusionCategory::Preference,
                reason: ExclusionReason::InvalidValue,
            });
            None
        }
    });
    let reasoning_effort = prefs.general_agent.reasoning_effort.and_then(|r| {
        let trimmed = r.trim();
        if matches!(trimmed, "low" | "medium" | "high" | "max" | "xhigh") {
            Some(trimmed.to_owned())
        } else {
            excluded.push(SanitizedExclusion {
                category: ExclusionCategory::Preference,
                reason: ExclusionReason::InvalidValue,
            });
            None
        }
    });

    let l1_budget_tokens = prefs.memory.l1_budget_tokens.map(|tok| tok.clamp(50, 2000));
    let cloud_idle_sleep_minutes = prefs
        .network
        .cloud_idle_sleep_minutes
        .map(|mins| mins.clamp(5, 1440));
    let cloud_max_awake_without_owner_minutes = prefs
        .network
        .cloud_max_awake_without_owner_minutes
        .map(|mins| mins.min(10_080));

    let sandbox_ceiling = prefs.agent.sandbox_ceiling.and_then(|value| {
        let trimmed = value.trim();
        if valid_sandbox_ceiling(trimmed) {
            Some(trimmed.to_owned())
        } else {
            excluded.push(SanitizedExclusion {
                category: ExclusionCategory::Preference,
                reason: ExclusionReason::DisallowedPreference,
            });
            None
        }
    });

    let mut configured_providers = Vec::new();
    for entry in prefs.configured_providers {
        if configured_providers.len() >= 16 {
            excluded.push(SanitizedExclusion {
                category: ExclusionCategory::ProviderRef,
                reason: ExclusionReason::BoundExceeded,
            });
            continue;
        }
        let provider_id = entry.provider_id.trim();
        if !valid_provider_id(provider_id) {
            excluded.push(SanitizedExclusion {
                category: ExclusionCategory::ProviderRef,
                reason: ExclusionReason::InvalidValue,
            });
            continue;
        }
        let default_model = entry.default_model.and_then(|m| {
            let trimmed = m.trim();
            if contains_secret_shaped_content(trimmed) {
                excluded.push(SanitizedExclusion {
                    category: ExclusionCategory::ProviderRef,
                    reason: ExclusionReason::SecretShapedContent,
                });
                None
            } else if valid_identifier(trimmed, 128) {
                Some(trimmed.to_owned())
            } else {
                None
            }
        });
        configured_providers.push(PortableProviderRef {
            provider_id: provider_id.to_owned(),
            default_model,
            resume_sessions: entry.resume_sessions,
        });
    }

    let mut acp_profiles = Vec::new();
    for entry in prefs.acp_profiles {
        if acp_profiles.len() >= 16 {
            excluded.push(SanitizedExclusion {
                category: ExclusionCategory::AcpProfile,
                reason: ExclusionReason::BoundExceeded,
            });
            continue;
        }
        let profile_id = entry.profile_id.trim();
        if !valid_provider_id(profile_id) {
            excluded.push(SanitizedExclusion {
                category: ExclusionCategory::AcpProfile,
                reason: ExclusionReason::InvalidValue,
            });
            continue;
        }
        let registry_id = entry.registry_id.and_then(|r| {
            let trimmed = r.trim();
            if contains_secret_shaped_content(trimmed) || contains_forbidden_path(trimmed) {
                excluded.push(SanitizedExclusion {
                    category: ExclusionCategory::AcpProfile,
                    reason: ExclusionReason::SecretShapedContent,
                });
                None
            } else if valid_identifier(trimmed, 128) {
                Some(trimmed.to_owned())
            } else {
                None
            }
        });
        acp_profiles.push(PortableAcpProfileRef {
            profile_id: profile_id.to_owned(),
            registry_id,
        });
    }

    PortablePreferences {
        general_agent: PortableGeneralAgentPrefs {
            agent_provider,
            model,
            reasoning_effort,
            capability_hints: prefs.general_agent.capability_hints,
        },
        memory: PortableMemoryPrefs {
            l1_budget_tokens,
            auto_extract: prefs.memory.auto_extract,
            librarian_enabled: prefs.memory.librarian_enabled,
        },
        network: PortableNetworkPrefs {
            cloud_idle_sleep_minutes,
            cloud_max_awake_without_owner_minutes,
        },
        agent: PortableAgentPrefs {
            resume_sessions: prefs.agent.resume_sessions,
            continue_after_restart: prefs.agent.continue_after_restart,
            sandbox_ceiling,
        },
        configured_providers,
        acp_profiles,
    }
}

pub fn sanitize_projection(
    projection: WarmBootstrapProjection,
) -> (WarmBootstrapProjection, Vec<SanitizedExclusion>) {
    let mut excluded = Vec::new();
    let clean = WarmBootstrapProjection {
        schema_version: projection.schema_version,
        account_id: projection.account_id.trim().to_owned(),
        owner_installation_id: projection.owner_installation_id.trim().to_owned(),
        target_installation_id: projection.target_installation_id.trim().to_owned(),
        creation_generation: projection.creation_generation.trim().to_owned(),
        recovery_revision: projection
            .recovery_revision
            .map(|r| r.trim().to_owned())
            .filter(|r| !r.is_empty()),
        identity: sanitize_identity(projection.identity, &mut excluded),
        wake_up_items: sanitize_wake_up_items(projection.wake_up_items, &mut excluded),
        skills: sanitize_skills(projection.skills, &mut excluded),
        preferences: sanitize_preferences(projection.preferences, &mut excluded),
    };
    (clean, excluded)
}

pub fn projection_digest(projection: &WarmBootstrapProjection) -> String {
    let bytes = serde_json::to_vec(projection).unwrap_or_default();
    format!("sha256:{:x}", Sha256::digest(&bytes))
}
