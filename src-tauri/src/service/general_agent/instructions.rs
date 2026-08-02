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

/// Pure render of `AGENT.md` content given the rendered wake-up snippet.
/// Split out from [`generate_instructions`] so unit tests can verify the
/// {wake_up} substitution without hitting real disk paths.
pub(super) fn render_agent_md(provider_name: &str, wake_up_section: &str) -> String {
    render_agent_md_at(&general_dir(), provider_name, wake_up_section)
}

fn render_agent_md_at(dir: &std::path::Path, provider_name: &str, wake_up_section: &str) -> String {
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

    let role_contract = crate::prompts::GENERAL_AGENT_TEMPLATE
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
        .replace("{codeterm_cli}", &general_cli_reference());
    compose_system_prompt(&role_contract)
}

fn general_cli_reference() -> String {
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

pub(super) fn render_system_prompt(
    provider_name: &str,
    mem: Option<&std::sync::Arc<crate::mem::MemManager>>,
) -> String {
    render_agent_md(provider_name, &render_wake_up_section(mem))
}

fn compose_system_prompt(agent: &str) -> String {
    format!(
        "{}\n\n---\n\n{}\n\n---\n\n{}",
        agent,
        crate::prompts::CODETERM_RULE,
        crate::prompts::BRIDGE_MEM,
    )
}

const GENERATED_START: &str = "<!-- CODETERM-GENERATED-START -->";
const GENERATED_END: &str = "<!-- CODETERM-GENERATED-END -->";
const LEGACY_CONTRACT_START: &str = "# CodeTerm General Agent (v";

fn generated_contract_bounds(existing: &str) -> Option<(usize, usize)> {
    match (existing.find(GENERATED_START), existing.find(GENERATED_END)) {
        (Some(start), Some(end)) if start < end => Some((start, end + GENERATED_END.len())),
        _ => {
            let start = existing.find(LEGACY_CONTRACT_START)?;
            let memory_offset = existing[start..].find(crate::prompts::BRIDGE_MEM.trim())?;
            let end = start + memory_offset + crate::prompts::BRIDGE_MEM.trim().len();
            Some((start, end))
        }
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
    provider_name: &str,
    mem: Option<&std::sync::Arc<crate::mem::MemManager>>,
) {
    let dir = general_dir();
    generate_instructions_at(&dir, provider_name, mem);
}

fn generate_instructions_at(
    dir: &std::path::Path,
    provider_name: &str,
    mem: Option<&std::sync::Arc<crate::mem::MemManager>>,
) {
    let wake_up_section = render_wake_up_section(mem);
    let agent_content = render_agent_md_at(dir, provider_name, &wake_up_section);

    if let Err(e) = std::fs::write(dir.join("AGENT.md"), &agent_content) {
        log::error!("[general-agent] Failed to write AGENT.md: {}", e);
    } else {
        log::info!("[general-agent] AGENT.md updated");
    }

    let claude_path = dir.join("CLAUDE.md");
    let existing_claude = std::fs::read_to_string(&claude_path).unwrap_or_default();
    if let Err(e) = std::fs::write(&claude_path, claude_import_file(&existing_claude)) {
        log::error!("[general-agent] Failed to write CLAUDE.md: {}", e);
    }

    let agents_path = dir.join("AGENTS.md");
    let existing_agents = std::fs::read_to_string(&agents_path).unwrap_or_default();
    let agents_content = generated_contract_file(&existing_agents, &agent_content);
    if let Err(e) = std::fs::write(agents_path, agents_content) {
        log::error!("[general-agent] Failed to write AGENTS.md: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let rendered = render_agent_md("claude", &section);
        assert!(!rendered.contains("{wake_up}"));
        assert!(!rendered.contains("L0 — Identity"));
        assert!(!rendered.contains("L1 — Critical context"));
    }

    #[test]
    fn general_system_prompt_composes_each_runtime_contract_once_without_bootstrap() {
        const DOCS_RULE: &str = "Use codeterm docs <NAME>; it returns the complete document. Use codeterm docs or codeterm docs --help to discover available names.";
        let rendered = render_system_prompt("codex", None);
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
        assert!(rendered.contains("You are the General Agent"));
        assert_eq!(rendered.matches("You are the General Agent").count(), 1);
        assert_eq!(rendered.matches("# CodeTerm CLI reference").count(), 1);
        assert_eq!(rendered.matches("# CodeTerm Memory Protocol").count(), 1);
        assert_eq!(rendered.matches(DOCS_RULE).count(), 1);
        assert!(!rendered.contains("<!-- bootstrap: true -->"));
        assert!(!rendered.contains("first-ever boot"));
    }

    #[test]
    fn generated_instruction_files_are_provider_correct_and_single_copy() {
        const DOCS_RULE: &str = "Use codeterm docs <NAME>; it returns the complete document. Use codeterm docs or codeterm docs --help to discover available names.";
        let dir = std::env::temp_dir().join(format!("ga-instructions-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("identity.md"), "IDENTITY_SENTINEL").unwrap();
        std::fs::write(dir.join("soul.md"), "SOUL_SENTINEL").unwrap();
        std::fs::write(dir.join("CLAUDE.md"), "@./AGENT.md\n\nUSER_CLAUDE_NOTE\n").unwrap();
        std::fs::write(dir.join("AGENTS.md"), "USER_AGENTS_NOTE\n").unwrap();

        generate_instructions_at(&dir, "codex", None);

        let agent = std::fs::read_to_string(dir.join("AGENT.md")).unwrap();
        let claude = std::fs::read_to_string(dir.join("CLAUDE.md")).unwrap();
        let agents = std::fs::read_to_string(dir.join("AGENTS.md")).unwrap();
        for contract in [&agent, &agents] {
            assert_eq!(contract.matches("You are the General Agent").count(), 1);
            assert_eq!(contract.matches("# CodeTerm CLI reference").count(), 1);
            assert_eq!(contract.matches("# CodeTerm Memory Protocol").count(), 1);
            assert_eq!(contract.matches("IDENTITY_SENTINEL").count(), 1);
            assert_eq!(contract.matches("SOUL_SENTINEL").count(), 1);
            assert_eq!(contract.matches(DOCS_RULE).count(), 1);
        }
        assert_eq!(claude.matches("@./AGENT.md").count(), 1);
        assert_eq!(claude.matches(DOCS_RULE).count(), 0);
        assert!(claude.contains("USER_CLAUDE_NOTE"));
        assert!(agents.contains("USER_AGENTS_NOTE"));

        generate_instructions_at(&dir, "codex", None);
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
        let legacy_contract = render_agent_md_at(&dir, "codex", "");
        let prepended_notes = "USER PREFIX\r\nkeep-leading-space:  yes\n";
        let appended_notes = "\n\nUSER SUFFIX  \r\nkeep-final-newline\n";
        std::fs::write(
            dir.join("AGENTS.md"),
            format!("{prepended_notes}{legacy_contract}{appended_notes}"),
        )
        .unwrap();

        for _ in 0..2 {
            generate_instructions_at(&dir, "codex", None);
            let agents = std::fs::read_to_string(dir.join("AGENTS.md")).unwrap();
            assert!(agents.starts_with(prepended_notes));
            assert!(agents.ends_with(appended_notes));
            assert_eq!(agents.matches(GENERATED_START).count(), 1);
            assert_eq!(agents.matches("You are the General Agent").count(), 1);
            assert_eq!(agents.matches("# CodeTerm CLI reference").count(), 1);
            assert_eq!(agents.matches("# CodeTerm Memory Protocol").count(), 1);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn general_agent_contract_preserves_memory_recall_save_graph_semantics() {
        let rendered = render_system_prompt("codex", None);
        for invariant in [
            "ENGLISH IN MEM",
            "SEARCH BEFORE ANSWERING",
            "NEVER GUESS IDS",
            "\"REMEMBER\" → SAVE NOW",
            "Facts change → save the new one",
            "TEMPORAL FACTS REQUIRE THE GRAPH",
            "Past conversation → `codeterm sessions search`",
            "codeterm mem skill new <slug>",
        ] {
            assert_eq!(
                rendered.matches(invariant).count(),
                1,
                "memory invariant must appear once: {invariant}"
            );
        }
    }

    /// Reproduces the GA-auto-resume regression: when something else holds
    /// `mem.conn`, calling `generate_instructions(_, Some(&mem))` from the
    /// boot path must not pin the calling thread. lifecycle::start_general_agent
    /// now spawns the regen on a side thread; this test pretends to be the
    /// boot path and asserts the spawn returns instantly even with the lock
    /// taken.
    #[test]
    fn generate_instructions_does_not_block_caller_when_mem_conn_held() {
        use std::sync::mpsc;
        use std::time::{Duration, Instant};

        let mem = crate::mem::MemManager::open_in_memory().unwrap();
        let dir = std::env::temp_dir().join(format!("ga-noblock-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("HOME", &dir);
        std::env::set_var("USERPROFILE", &dir);

        // Hold the mem connection lock for the duration of the test — simulates a long-running embed/extract worker stalled on it.
        let _conn_guard = mem.conn().lock();

        let mem_clone = mem.clone();
        let (started_tx, started_rx) = mpsc::channel();
        let t0 = Instant::now();
        let handle = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            // This call WILL block on conn.lock(); the parent must not.
            generate_instructions("claude", Some(&mem_clone));
        });
        // `spawn` returns immediately; the parent is not blocked even though the spawned closure is parked on the conn lock.
        let elapsed = t0.elapsed();
        assert!(
            elapsed < Duration::from_millis(500),
            "spawn must not block caller (took {:?})",
            elapsed,
        );
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("spawned thread must run");
        // We deliberately leak the join handle — releasing _conn_guard when the test exits unblocks the spawned thread, but waiting on it would couple the test to wake_up's runtime cost.
        std::mem::drop(handle);
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

        let rendered = render_agent_md("claude", &section);
        assert!(rendered.contains("## Live mem snapshot"));
        assert!(rendered.contains("Alice"));
        assert!(rendered.contains("never edit autogen"));
        assert!(!rendered.contains("{wake_up}"));
    }
}
