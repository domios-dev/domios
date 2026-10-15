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
            let source = format!("file:{}", &file_path);
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
    let version = env!("CARGO_PKG_VERSION");
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
mod tests {
    use super::*;

    #[test]
    fn capability_hint_fragment_reaches_general_prompt_only_in_window_and_when_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let base = render_agent_md_at(dir.path(), "OPAQUE_PROVIDER", "");
        let capabilities = [super::super::capability_hints::Capability {
            id: "OPAQUE_CAPABILITY".into(),
            purpose: "OPAQUE_PURPOSE".into(),
        }];
        let fragment = super::super::capability_hints::build(&capabilities, 0, 0, true).unwrap();
        let inside = compose_general_prompt_at(dir.path(), "OPAQUE_PROVIDER", "", Some(&fragment));
        assert_eq!(inside, format!("{base}{fragment}"));
        for (now, enabled) in [
            (super::super::capability_hints::FIRST_WEEK_SECONDS, true),
            (0, false),
        ] {
            let absent = super::super::capability_hints::build(&capabilities, now, 0, enabled);
            assert_eq!(
                compose_general_prompt_at(dir.path(), "OPAQUE_PROVIDER", "", absent.as_deref()),
                base
            );
        }
        assert_eq!(
            append_capability_hints(base.clone(), Some("OPAQUE_FRAGMENT")),
            format!("{base}OPAQUE_FRAGMENT")
        );
    }

    #[test]
    fn general_agent_memory_management_fragment_is_composed_once() {
        let dir = tempfile::tempdir().unwrap();
        let rendered = render_agent_md_at(dir.path(), "OPAQUE_PROVIDER", "OPAQUE_WAKE_UP");
        let fragment = crate::prompts::MEM_MANAGEMENT.trim();
        assert!(!fragment.is_empty());
        assert_eq!(rendered.matches(fragment).count(), 1);
        assert_eq!(rendered.matches("OPAQUE_PROVIDER").count(), 1);
        assert_eq!(rendered.matches("OPAQUE_WAKE_UP").count(), 1);
        for slot in [
            "{version}",
            "{platform}",
            "{hostname}",
            "{provider_name}",
            "{general_dir}",
            "{secrets_dir}",
            "{skills_dir}",
            "{identity}",
            "{soul}",
            "{wake_up}",
            "{codeterm_cli}",
            "{interactive_child_pty_operation}",
        ] {
            assert!(!rendered.contains(slot), "unsubstituted slot {slot}");
        }
    }

    #[test]
    fn unreadable_user_file_is_never_rebuilt_from_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("CLAUDE.md");
        let unreadable = vec![b'#', 0xff, 0xfe, b'\n'];
        std::fs::write(&path, &unreadable).unwrap();

        rewrite_user_file(&path, |existing| format!("{existing}generated"));

        assert_eq!(std::fs::read(&path).unwrap(), unreadable);
    }

    #[test]
    fn test_agent_md_import_in_claude_md() {
        let claude_content = "@./AGENT.md\n\n# Notes\n\nYour personal notes and memory go here.\n";
        assert!(claude_content.contains("@./AGENT.md"));
        assert!(claude_content.contains("Notes"));
    }

    #[test]
    fn test_existing_claude_md_gets_import() {
        let existing = "# My notes\nSome user content\n";
        assert!(!existing.contains("@./AGENT.md"));
        let updated = format!("@./AGENT.md\n\n{}", existing);
        assert!(updated.contains("@./AGENT.md"));
        assert!(updated.contains("Some user content"));
    }

    #[test]
    fn test_agent_template_no_markers() {
        let template = crate::prompts::GENERAL_AGENT_TEMPLATE;
        assert!(!template.contains("CODETERM-AUTO-START"));
        assert!(!template.contains("CODETERM-AUTO-END"));
    }

    #[test]
    fn test_template_has_wake_up_placeholder() {
        let template = crate::prompts::GENERAL_AGENT_TEMPLATE;
        assert!(
            template.contains("{wake_up}"),
            "general_agent.md template must declare {{wake_up}} placeholder"
        );
    }

    #[test]
    fn test_bootstrap_template_renders_general_dir() {
        let rendered = render_bootstrap_instructions();

        assert!(
            !rendered.contains("{general_dir}"),
            "bootstrap instructions must not leak the literal placeholder"
        );
        assert!(
            rendered.contains("bootstrap.md"),
            "bootstrap instructions should still tell the GA which file to delete"
        );
        assert!(
            rendered.contains("Remove-Item") && rendered.contains("rm "),
            "bootstrap cleanup must remain cross-platform"
        );
    }

    #[test]
    fn test_wake_up_section_empty_when_mem_none() {
        let section = render_wake_up_section(None);
        assert!(section.is_empty());
        let dir = tempfile::tempdir().unwrap();
        let rendered = render_agent_md_at(dir.path(), "claude", &section);
        assert!(!rendered.contains("{wake_up}"));
        assert!(!rendered.contains("L0 — Identity"));
        assert!(!rendered.contains("L1 — Critical context"));
    }

    #[test]
    fn general_agent_shared_cli_fragment_is_injected_once_without_unresolved_slot() {
        let dir = tempfile::tempdir().unwrap();
        let rendered = render_agent_md_at(dir.path(), "supplied-provider", "");
        let shared = cli_without_memory_section();
        assert_eq!(rendered.matches(shared.trim()).count(), 1);
        assert!(!rendered.contains("{codeterm_cli}"));
    }

    #[test]
    fn general_agent_receives_shared_child_pty_operation() {
        let dir = tempfile::tempdir().unwrap();
        let rendered = render_agent_md_at(dir.path(), "claude", "");
        let operation = crate::prompts::INTERACTIVE_CHILD_PTY_OPERATION.trim();

        assert_eq!(rendered.matches(operation).count(), 1);
        assert!(!rendered.contains("{interactive_child_pty_operation}"));
    }

    #[test]
    fn general_system_prompt_composes_each_runtime_contract_once_without_bootstrap() {
        let dir = tempfile::tempdir().unwrap();
        let rendered = compose_general_prompt_at(dir.path(), "codex", "", None);
        assert_eq!(
            rendered
                .matches(crate::prompts::CODETERM_RULE.trim())
                .count(),
            1
        );
        assert_eq!(
            rendered.matches(crate::prompts::BRIDGE_MEM.trim()).count(),
            1
        );
        assert_eq!(
            rendered
                .matches(crate::prompts::AGENT_CODETERM_PRIMER.trim())
                .count(),
            1
        );
        assert_eq!(
            rendered
                .matches(cli_without_memory_section().trim())
                .count(),
            1
        );
        assert!(!rendered.contains("<!-- bootstrap: true -->"));
        assert!(!rendered.contains(crate::prompts::BOOTSTRAP_INSTRUCTIONS_TEMPLATE.trim()));
    }

    #[test]
    fn generated_instruction_files_are_provider_correct_and_single_copy() {
        let dir = std::env::temp_dir().join(format!("ga-instructions-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("identity.md"), "IDENTITY_SENTINEL").unwrap();
        std::fs::write(dir.join("soul.md"), "SOUL_SENTINEL").unwrap();
        std::fs::write(dir.join("CLAUDE.md"), "@./AGENT.md\n\nUSER_CLAUDE_NOTE\n").unwrap();
        std::fs::write(dir.join("AGENTS.md"), "USER_AGENTS_NOTE\n").unwrap();

        generate_instructions_at(&dir, "codex", None, false);

        let agent = std::fs::read_to_string(dir.join("AGENT.md")).unwrap();
        let claude = std::fs::read_to_string(dir.join("CLAUDE.md")).unwrap();
        let agents = std::fs::read_to_string(dir.join("AGENTS.md")).unwrap();
        let pad_rule = crate::prompts::AGENT_PAD_RULE.trim();
        assert!(!pad_rule.is_empty(), "an empty fragment proves nothing");
        for contract in [&agent, &agents] {
            assert_eq!(
                contract
                    .matches(crate::prompts::AGENT_CODETERM_PRIMER.trim())
                    .count(),
                1
            );
            assert_eq!(
                contract.matches(pad_rule).count(),
                1,
                "the written instruction file must carry the AgentPad rule exactly once"
            );
            assert_eq!(
                contract
                    .matches(cli_without_memory_section().trim())
                    .count(),
                1
            );
            assert_eq!(
                contract.matches(crate::prompts::BRIDGE_MEM.trim()).count(),
                1
            );
            assert_eq!(contract.matches("IDENTITY_SENTINEL").count(), 1);
            assert_eq!(contract.matches("SOUL_SENTINEL").count(), 1);
        }
        assert_eq!(claude.matches("@./AGENT.md").count(), 1);
        assert_eq!(claude.matches(crate::prompts::BRIDGE_MEM.trim()).count(), 0);
        assert!(claude.contains("USER_CLAUDE_NOTE"));
        assert!(agents.contains("USER_AGENTS_NOTE"));

        generate_instructions_at(&dir, "codex", None, false);
        let agents = std::fs::read_to_string(dir.join("AGENTS.md")).unwrap();
        assert_eq!(agents.matches(GENERATED_START).count(), 1);
        assert_eq!(agents.matches("USER_AGENTS_NOTE").count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn legacy_unmarked_agents_md_upgrades_without_preserving_generated_contract_as_notes() {
        let dir = std::env::temp_dir().join(format!("ga-legacy-agents-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("identity.md"), "LEGACY_IDENTITY").unwrap();
        std::fs::write(dir.join("soul.md"), "LEGACY_SOUL").unwrap();
        let legacy_contract = format!(
            "{LEGACY_CONTRACT_PRIMER}\n\n---\n\n{LEGACY_CONTRACT_START}0.0.0)\nOPAQUE_LEGACY_BODY\n\n{}",
            LEGACY_CONTRACT_MEMORY,
        );
        let prepended_notes = "USER PREFIX\r\nkeep-leading-space:  yes\n";
        let appended_notes = "\n\nUSER SUFFIX  \r\nkeep-final-newline\n";
        assert_eq!(
            generated_contract_file(
                &format!("{prepended_notes}{legacy_contract}{appended_notes}"),
                "OPAQUE_CURRENT_CONTRACT",
            ),
            format!("{prepended_notes}{GENERATED_START}\nOPAQUE_CURRENT_CONTRACT\n{GENERATED_END}{appended_notes}"),
        );
        std::fs::write(
            dir.join("AGENTS.md"),
            format!("{prepended_notes}{legacy_contract}{appended_notes}"),
        )
        .unwrap();

        for _ in 0..2 {
            generate_instructions_at(&dir, "codex", None, false);
            let agents = std::fs::read_to_string(dir.join("AGENTS.md")).unwrap();
            assert!(agents.starts_with(prepended_notes));
            assert!(agents.ends_with(appended_notes));
            assert_eq!(agents.matches(GENERATED_START).count(), 1);
            assert_eq!(
                agents.matches(cli_without_memory_section().trim()).count(),
                1
            );
            assert_eq!(agents.matches(crate::prompts::BRIDGE_MEM.trim()).count(), 1);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn role_authority_stripping_preserves_notes_and_accepts_only_an_exact_suffix() {
        let authority = "opaque-authority";
        let original = format!("opaque-notes:{authority}\nopaque-primer\nopaque-plugins");
        let current = format!("{original}\n\n{authority}");
        assert_eq!(without_role_authority(&current, authority), original);
        assert_eq!(without_role_authority(&original, authority), original);
        let foreign = format!("{current}\nopaque-user-note");
        assert_eq!(without_role_authority(&foreign, authority), foreign);
        assert_eq!(without_role_authority(authority, authority), authority);
    }

    #[test]
    fn current_unmarked_generated_contract_preserves_notes_without_duplicate_primer() {
        let dir = tempfile::tempdir().unwrap();
        let contract = render_agent_md_at(dir.path(), "codex", "");
        let prefix = "OPAQUE_USER_PREFIX\r\n";
        let suffix = "\nOPAQUE_USER_SUFFIX  \r\n";
        let rewritten =
            generated_contract_file(&format!("{prefix}{contract}{suffix}"), "CURRENT_CONTRACT");

        assert_eq!(
            rewritten,
            format!("{prefix}{GENERATED_START}\nCURRENT_CONTRACT\n{GENERATED_END}{suffix}")
        );
    }

    #[test]
    fn general_role_fragment_is_opaque_and_follows_the_shared_primer() {
        let role = "opaque-general-role";
        let prompt = compose_system_prompt_with_plugins(role, Some("opaque-plugin-fragment"));
        assert_eq!(prompt.matches(role).count(), 1);
        assert!(prompt.find(crate::prompts::AGENT_CODETERM_PRIMER.trim()) < prompt.find(role));
        assert!(prompt.find("opaque-plugin-fragment") < prompt.find(role));
        assert_eq!(
            prompt
                .matches(crate::service::orchestrator::ROLE_AUTHORITY)
                .count(),
            1
        );
    }

    #[test]
    fn built_in_plugins_section_follows_the_primer_and_strips_with_the_contract() {
        const SECTION: &str = "Built-in plugins:\n- OPAQUE_CORE_LINE_q7";
        let dir = tempfile::tempdir().unwrap();
        let role = render_role_contract_at(dir.path(), "codex", "");
        let plain = compose_system_prompt_with_plugins(&role, None);
        assert!(!plain.contains("Built-in plugins:"));
        let with = compose_system_prompt_with_plugins(&role, Some(SECTION));
        assert!(with.starts_with(crate::prompts::AGENT_CODETERM_PRIMER));
        assert!(with.find(SECTION) < with.find(&role));
        assert_eq!(
            with.matches(crate::service::orchestrator::ROLE_AUTHORITY)
                .count(),
            1
        );
        assert_eq!(with.replacen(&format!("\n{SECTION}\n"), "", 1), plain);

        let prefix = "OPAQUE_USER_PREFIX\r\n";
        let suffix = "\nOPAQUE_USER_SUFFIX  \r\n";
        let rewritten =
            generated_contract_file(&format!("{prefix}{with}{suffix}"), "CURRENT_CONTRACT");
        assert_eq!(
            rewritten,
            format!("{prefix}{GENERATED_START}\nCURRENT_CONTRACT\n{GENERATED_END}{suffix}")
        );
        let marked = generated_contract_file("", &with);
        assert_eq!(
            generated_contract_file(&marked, &plain),
            generated_contract_file("", &plain)
        );
    }

    #[test]
    fn renamed_unmarked_generated_contract_preserves_notes_without_duplicate_body() {
        let dir = tempfile::tempdir().unwrap();
        let original = render_agent_md_at(dir.path(), "codex", "");
        let renamed = original.replace("CodeTerm", "Domios");
        let prefix = "USER_PREFIX\r\n";
        let suffix = "\nUSER_SUFFIX  \r\n";
        let existing = format!("{prefix}{renamed}{suffix}");
        let replacement = "OPAQUE_CURRENT_CONTRACT";

        let rewritten = generated_contract_file(&existing, replacement);

        assert_eq!(
            rewritten,
            format!("{prefix}{GENERATED_START}\n{replacement}\n{GENERATED_END}{suffix}")
        );
        assert_eq!(generated_contract_file(&rewritten, replacement), rewritten);
    }

    #[test]
    fn incomplete_or_foreign_unmarked_contract_is_preserved_as_notes() {
        let current_heading = crate::prompts::GENERAL_AGENT_TEMPLATE
            .split("{version}")
            .next()
            .unwrap();
        for existing in [
            format!("{current_heading}0.0.0)\nOPAQUE_INCOMPLETE_BODY\n"),
            format!("# FOREIGN_HEADER\n{}\n", crate::prompts::BRIDGE_MEM.trim()),
        ] {
            assert_eq!(
                generated_contract_file(&existing, "OPAQUE_REPLACEMENT"),
                format!("{GENERATED_START}\nOPAQUE_REPLACEMENT\n{GENERATED_END}\n\n{existing}")
            );
        }
    }

    #[test]
    fn nonadjacent_primer_is_preserved_as_user_notes() {
        let heading = crate::prompts::GENERAL_AGENT_TEMPLATE
            .split("{version}")
            .next()
            .unwrap();
        let notes = format!(
            "{}\nOPAQUE_INTERVENING_NOTE\n",
            crate::prompts::AGENT_CODETERM_PRIMER
        );
        let body = format!(
            "{heading}0.0.0)\nOPAQUE_BODY\n{}",
            crate::prompts::BRIDGE_MEM
        );
        assert_eq!(
            generated_contract_file(&format!("{notes}{body}"), "OPAQUE_REPLACEMENT"),
            format!("{notes}{GENERATED_START}\nOPAQUE_REPLACEMENT\n{GENERATED_END}")
        );
    }

    #[test]
    fn general_agent_contract_preserves_memory_recall_save_graph_semantics() {
        let dir = tempfile::tempdir().unwrap();
        let rendered = compose_general_prompt_at(dir.path(), "codex", "", None);
        assert_eq!(
            rendered.matches(crate::prompts::BRIDGE_MEM.trim()).count(),
            1,
            "the memory protocol reaches the General Agent exactly once"
        );
        for command in [
            "`codeterm mem search`",
            "`codeterm mem save`",
            "`codeterm sessions search`",
            "`codeterm mem skill new <slug>`",
        ] {
            assert!(
                crate::prompts::BRIDGE_MEM.contains(command),
                "the memory protocol names the CLI command {command}"
            );
        }
    }

    #[test]
    fn generate_instructions_does_not_block_caller_when_mem_conn_held() {
        use std::sync::mpsc;
        use std::time::{Duration, Instant};

        let mem = crate::mem::MemManager::open_in_memory().unwrap();
        let dir = std::env::temp_dir().join(format!("ga-noblock-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&dir).unwrap();

        let _conn_guard = mem.conn().lock();

        let mem_clone = mem.clone();
        let output_dir = dir.clone();
        let (started_tx, started_rx) = mpsc::channel();
        let t0 = Instant::now();
        let handle = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            generate_instructions_at(&output_dir, "claude", Some(&mem_clone), false);
        });
        let elapsed = t0.elapsed();
        assert!(
            elapsed < Duration::from_millis(500),
            "spawn must not block caller (took {:?})",
            elapsed,
        );
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("spawned thread must run");
        drop(_conn_guard);
        handle.join().expect("instruction generation must complete");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_wake_up_injects_identity_into_agent_md() {
        let mem = crate::mem::MemManager::open_in_memory().unwrap();
        mem.save_replace(crate::mem::NewMemItem {
            kind: crate::mem::MemKind::Preference,
            scope: crate::mem::MemScope::Global,
            title: "identity".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "I am Alice, the General Agent on this dev box.".into(),
            source: None,
            importance: 5.0,
            source_path: None,
            pinned: false,
        })
        .unwrap();
        mem.save(crate::mem::NewMemItem {
            kind: crate::mem::MemKind::Rule,
            scope: crate::mem::MemScope::Global,
            title: "never edit autogen".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "Never edit CLAUDE.md or AGENTS.md by hand.".into(),
            source: None,
            importance: 5.0,
            source_path: None,
            pinned: false,
        })
        .unwrap();

        let section = render_wake_up_section(Some(&mem));
        assert!(
            section.contains("## Live mem snapshot"),
            "section must have header: {}",
            section
        );
        assert!(
            section.contains("Alice"),
            "section must contain identity body: {}",
            section
        );

        let dir = tempfile::tempdir().unwrap();
        let rendered = render_agent_md_at(dir.path(), "claude", &section);
        assert!(rendered.contains("## Live mem snapshot"));
        assert!(rendered.contains("Alice"));
        assert!(rendered.contains("never edit autogen"));
        assert!(!rendered.contains("{wake_up}"));
    }
}
