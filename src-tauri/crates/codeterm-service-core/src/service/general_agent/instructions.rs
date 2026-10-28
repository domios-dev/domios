//! AGENT.md / CLAUDE.md / AGENTS.md / bootstrap.md generation.
//!
//! Templates live in `crate::prompts`; this module substitutes runtime values
//! (version, hostname, identity/soul, live mem snapshot) and writes the four
//! files under [`super::general_dir`].

use super::{general_dir, secrets_dir};

/// Sync identity / soul markdown files into `mem` as lightweight index entries.
/// The full content stays on disk; mem stores a 200-char summary for FTS plus
/// the file path in `source`. Vectors are built from the file content by the
/// reindex daemon.
pub fn sync_identity_to_mem(mem: &std::sync::Arc<crate::mem::MemManager>) {
    crate::mem::store::with_default_system_author("identity-sync", || {
        let dir = general_dir();

        let files: &[(&str, &str, &str, &[&str])] = &[
            (
                "identity.md",
                "identity",
                "User identity — name, role, location, communication style",
                &["identity"],
            ),
            (
                "soul.md",
                "soul",
                "Agent personality — character, tone, behavior rules",
                &["soul"],
            ),
        ];

        for (filename, title, summary, tags) in files {
            let path = dir.join(filename);
            if let Ok(content) = std::fs::read_to_string(&path) {
                let trimmed = content.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let short: String = trimmed.chars().take(200).collect();
                let file_path = path.to_string_lossy().to_string();
                let source = format!("file:{}", file_path);
                let _ = mem.save_replace(crate::mem::store::NewMemItem {
                    kind: crate::mem::store::MemKind::Preference,
                    scope: crate::mem::store::MemScope::Global,
                    title: title.to_string(),
                    summary: Some(summary.to_string()),
                    area: None,
                    tags: tags.iter().map(|t| t.to_string()).collect(),
                    body: short,
                    source: Some(source),
                    importance: 5.0,
                    source_path: Some(file_path),
                    pinned: false,
                });
            }
        }
    })
}

fn render_agent_md_at(dir: &std::path::Path, provider_name: &str, wake_up_section: &str) -> String {
    compose_system_prompt(&render_role_contract_at(
        dir,
        provider_name,
        wake_up_section,
    ))
}

fn render_role_contract_at(
    dir: &std::path::Path,
    provider_name: &str,
    wake_up_section: &str,
) -> String {
    let version = codeterm_build_info::version();
    let platform = if cfg!(target_os = "windows") {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else {
        "Linux"
    };
    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let identity = std::fs::read_to_string(dir.join("identity.md")).unwrap_or_default();
    let soul = std::fs::read_to_string(dir.join("soul.md")).unwrap_or_default();

    let identity_section = if identity.trim().is_empty() {
        "No identity configured yet. Ask the user who they are during bootstrap.".to_string()
    } else {
        identity.trim().to_string()
    };

    let soul_section = if soul.trim().is_empty() {
        String::new()
    } else {
        format!("\n## Soul\n\n{}\n", soul.trim())
    };

    crate::prompts::GENERAL_AGENT_TEMPLATE
        .replace("{version}", version)
        .replace("{platform}", platform)
        .replace("{hostname}", &hostname)
        .replace("{provider_name}", provider_name)
        .replace("{general_dir}", &dir.to_string_lossy())
        .replace("{secrets_dir}", &secrets_dir().to_string_lossy())
        .replace(
            "{skills_dir}",
            &crate::paths::data_dir().join("skills").to_string_lossy(),
        )
        .replace("{identity}", &identity_section)
        .replace("{soul}", &soul_section)
        .replace("{wake_up}", wake_up_section)
        .replace("{codeterm_cli}", &general_cli_reference())
        .replace(
            "{interactive_child_pty_operation}",
            crate::prompts::INTERACTIVE_CHILD_PTY_OPERATION.trim(),
        )
}

fn general_cli_reference() -> String {
    format!(
        "{}\n{}",
        cli_without_memory_section(),
        crate::prompts::AGENT_PAD_RULE.trim()
    )
}

/// The GA receives memory guidance from its own sections, so the shared CLI
/// reference drops that block. Falls back to the whole reference if the section
/// headings move — losing the trim is harmless, losing the text would not be.
fn cli_without_memory_section() -> String {
    let cli = crate::prompts::CODETERM_CLI;
    let Some(memory_start) = cli.find("## Memory (mem)") else {
        return cli.to_string();
    };
    let Some(platform_offset) = cli[memory_start..].find("## Platform notes") else {
        return cli.to_string();
    };
    let platform_start = memory_start + platform_offset;
    format!("{}{}", &cli[..memory_start], &cli[platform_start..])
}

pub(crate) fn render_system_prompt(
    capability_hints: bool,
    provider_name: &str,
    mem: Option<&std::sync::Arc<crate::mem::MemManager>>,
) -> String {
    compose_general_prompt_at(
        &general_dir(),
        provider_name,
        &render_wake_up_section(mem),
        runtime_capability_hints(&general_dir(), capability_hints).as_deref(),
    )
}

fn runtime_capability_hints(dir: &std::path::Path, enabled: bool) -> Option<String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    let first_seen = match super::state::first_seen_at(dir, now) {
        Ok(first_seen) => first_seen,
        Err(error) => {
            log::warn!("[general-agent] first-seen state unavailable: {error}");
            return None;
        }
    };
    super::capability_hints::build(
        &crate::service::plugin_prompt::capability_hints(),
        now,
        first_seen,
        enabled,
    )
}

fn compose_general_prompt_at(
    dir: &std::path::Path,
    provider_name: &str,
    wake_up: &str,
    fragment: Option<&str>,
) -> String {
    append_capability_hints(render_agent_md_at(dir, provider_name, wake_up), fragment)
}

fn append_capability_hints(mut prompt: String, fragment: Option<&str>) -> String {
    if let Some(fragment) = fragment {
        prompt.push_str(fragment);
    }
    prompt
}

fn compose_system_prompt(agent: &str) -> String {
    compose_system_prompt_with_plugins(
        agent,
        crate::service::plugin_prompt::built_in_plugins_section().as_deref(),
    )
}

fn compose_system_prompt_with_plugins(agent: &str, built_in_plugins: Option<&str>) -> String {
    let mut primer = match built_in_plugins {
        Some(section) => format!("{}\n{section}\n", crate::prompts::AGENT_CODETERM_PRIMER),
        None => crate::prompts::AGENT_CODETERM_PRIMER.to_string(),
    };
    primer.push_str("\n\n");
    primer.push_str(crate::service::orchestrator::ROLE_AUTHORITY);
    format!(
        "{}\n\n---\n\n{}\n\n---\n\n{}\n\n---\n\n{}\n\n{}",
        primer,
        agent,
        crate::prompts::CODETERM_RULE,
        crate::prompts::MEM_MANAGEMENT.trim(),
        crate::prompts::BRIDGE_MEM,
    )
}

const GENERATED_START: &str = "<!-- CODETERM-GENERATED-START -->";
const GENERATED_END: &str = "<!-- CODETERM-GENERATED-END -->";
const LEGACY_CONTRACT_START: &str = "# CodeTerm General Agent (v";
const LEGACY_CONTRACT_MEMORY: &str = include_str!("legacy_contract_memory.txt");
const LEGACY_CONTRACT_PRIMER: &str = include_str!("legacy_contract_primer.txt");

fn generated_contract_bounds(existing: &str) -> Option<(usize, usize)> {
    match (existing.find(GENERATED_START), existing.find(GENERATED_END)) {
        (Some(start), Some(end)) if start < end => Some((start, end + GENERATED_END.len())),
        _ => unmarked_contract_bounds(existing),
    }
}

fn unmarked_contract_bounds(existing: &str) -> Option<(usize, usize)> {
    let current_heading = crate::prompts::GENERAL_AGENT_TEMPLATE
        .split("{version}")
        .next()?;
    let heading = [current_heading, LEGACY_CONTRACT_START]
        .into_iter()
        .filter_map(|heading| existing.find(heading))
        .min()?;
    let end = [
        crate::prompts::BRIDGE_MEM,
        LEGACY_CONTRACT_MEMORY,
        crate::prompts::BRIDGE_MEM.trim(),
        LEGACY_CONTRACT_MEMORY.trim(),
    ]
    .into_iter()
    .find_map(|memory| {
        let offset = existing[heading..].find(memory)?;
        Some(heading + offset + memory.len())
    })?;
    let start = [
        crate::prompts::AGENT_CODETERM_PRIMER,
        LEGACY_CONTRACT_PRIMER,
    ]
    .into_iter()
    .find_map(|primer| {
        let prelude = existing[..heading].strip_suffix("\n\n---\n\n")?;
        let prelude = without_role_authority(prelude, crate::service::orchestrator::ROLE_AUTHORITY);
        without_built_in_plugins_section(prelude)
            .strip_suffix(primer)
            .map(|notes| notes.len())
    })
    .unwrap_or(heading);
    Some((start, end))
}

fn without_role_authority<'a>(prelude: &'a str, authority: &str) -> &'a str {
    prelude
        .strip_suffix(&format!("\n\n{authority}"))
        .unwrap_or(prelude)
}

fn without_built_in_plugins_section(prelude: &str) -> &str {
    let marker = format!(
        "\n{}\n",
        codeterm_plugin::agent_prompt_lines::BUILT_IN_PLUGINS_HEADING
    );
    match prelude.rfind(&marker) {
        Some(index)
            if prelude[index + marker.len()..]
                .lines()
                .all(|line| line.starts_with("- ")) =>
        {
            &prelude[..index]
        }
        _ => prelude,
    }
}

fn generated_contract_file(existing: &str, contract: &str) -> String {
    let generated = format!("{GENERATED_START}\n{contract}\n{GENERATED_END}");
    if let Some((start, end)) = generated_contract_bounds(existing) {
        return format!("{}{generated}{}", &existing[..start], &existing[end..]);
    }
    if existing.is_empty() {
        return format!("{generated}\n");
    }
    format!("{generated}\n\n{existing}")
}

fn claude_import_file(existing: &str) -> String {
    let notes = existing
        .lines()
        .filter(|line| line.trim() != "@./AGENT.md")
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    if notes.is_empty() {
        "@./AGENT.md\n".to_string()
    } else {
        format!("@./AGENT.md\n\n{notes}\n")
    }
}

/// Render the L0+L1 wake-up payload as a markdown section, or empty when
/// mem is unavailable / failed. On error we log and substitute empty so GA
/// bootstrap never fails because of memory.
pub(super) fn render_wake_up_section(
    mem: Option<&std::sync::Arc<crate::mem::MemManager>>,
) -> String {
    let Some(m) = mem else { return String::new() };
    match m.wake_up(&crate::mem::WakeUpOptions::default()) {
        Ok(text) => format!("\n## Live mem snapshot\n\n{}\n", text.trim()),
        Err(e) => {
            log::warn!(
                "[general-agent] wake-up render failed: {} — injecting empty section",
                e
            );
            String::new()
        }
    }
}

pub(super) fn render_bootstrap_instructions() -> String {
    crate::prompts::BOOTSTRAP_INSTRUCTIONS_TEMPLATE
        .replace("{general_dir}", &general_dir().to_string_lossy())
}

pub(super) fn ensure_bootstrap_instructions() -> Result<(), String> {
    let path = general_dir().join("bootstrap.md");
    if path.exists() {
        return Ok(());
    }
    std::fs::write(path, render_bootstrap_instructions()).map_err(|error| error.to_string())
}

pub fn generate_instructions(
    capability_hints: bool,
    provider_name: &str,
    mem: Option<&std::sync::Arc<crate::mem::MemManager>>,
) {
    let dir = general_dir();
    generate_instructions_at(&dir, provider_name, mem, capability_hints);
}

fn generate_instructions_at(
    dir: &std::path::Path,
    provider_name: &str,
    mem: Option<&std::sync::Arc<crate::mem::MemManager>>,
    capability_hints: bool,
) {
    let wake_up_section = render_wake_up_section(mem);
    let agent_content = compose_general_prompt_at(
        dir,
        provider_name,
        &wake_up_section,
        runtime_capability_hints(dir, capability_hints).as_deref(),
    );

    if let Err(e) = std::fs::write(dir.join("AGENT.md"), &agent_content) {
        log::error!("[general-agent] Failed to write AGENT.md: {}", e);
    } else {
        log::info!("[general-agent] AGENT.md updated");
    }

    rewrite_user_file(&dir.join("CLAUDE.md"), claude_import_file);
    rewrite_user_file(&dir.join("AGENTS.md"), |existing| {
        generated_contract_file(existing, &agent_content)
    });
}

/// These files carry the user's notes, so one that exists but cannot be read
/// is left untouched instead of being rebuilt from an empty base.
fn rewrite_user_file(path: &std::path::Path, render: impl FnOnce(&str) -> String) {
    let existing = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            log::error!(
                "[general-agent] {} is unreadable ({}); left untouched",
                path.display(),
                e
            );
            return;
        }
    };
    if let Err(e) = std::fs::write(path, render(&existing)) {
        log::error!("[general-agent] Failed to write {}: {}", path.display(), e);
    }
}

#[cfg(test)]
mod tests;
