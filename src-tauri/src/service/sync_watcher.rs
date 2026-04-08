//! Regenerates `AGENTS.md` on `MEMORY.md` change + installs mem bridge
//! rule on every registered provider at startup and on plugin changes.
//! See `docs/plans/mem-architecture.md` Phase 3.0.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};

use crate::agent::registry::AgentRegistry;

pub fn start(codeterm_dir: PathBuf, agent_registry: Arc<AgentRegistry>) {
    let general_dir = codeterm_dir.join("general");
    let memory_path = general_dir.join("MEMORY.md");

    install_bridge_rules_for_all(&agent_registry);
    if general_dir.exists() {
        regenerate_agents_md(&general_dir);
    }

    std::thread::Builder::new()
        .name("sync-watcher".into())
        .spawn(move || {
            let (tx, rx) = std::sync::mpsc::channel();

            let mut debouncer = match new_debouncer(Duration::from_millis(300), tx) {
                Ok(d) => d,
                Err(e) => {
                    log::error!("[sync-watcher] Failed to create debouncer: {}", e);
                    return;
                }
            };

            // Watch general directory (for MEMORY.md changes)
            if general_dir.exists() {
                if let Err(e) = debouncer.watcher().watch(&general_dir, notify::RecursiveMode::NonRecursive) {
                    log::error!("[sync-watcher] Failed to watch general dir: {}", e);
                } else {
                    log::info!("[sync-watcher] Watching {}", general_dir.display());
                }
            }

            loop {
                match rx.recv() {
                    Ok(Ok(events)) => {
                        let mut memory_changed = false;

                        for event in &events {
                            if event.kind == DebouncedEventKind::Any && event.path == memory_path {
                                memory_changed = true;
                            }
                        }

                        if memory_changed {
                            log::info!("[sync-watcher] MEMORY.md changed, regenerating AGENTS.md");
                            regenerate_agents_md(&general_dir);
                        }
                    }
                    Ok(Err(errors)) => {
                        log::warn!("[sync-watcher] Watch error: {:?}", errors);
                    }
                    Err(_) => {
                        log::info!("[sync-watcher] Channel closed, stopping");
                        break;
                    }
                }
            }
        })
        .expect("Failed to spawn sync-watcher thread");
}

/// Install the mem bridge payload on every registered provider.
/// Idempotent — safe to call on startup and on every plugin activation.
pub fn install_bridge_rules_for_all(registry: &AgentRegistry) {
    let payload = crate::prompts::BRIDGE_MEM;
    let provider_ids = registry.provider_ids();
    let mut installed = 0usize;
    let mut failed = 0usize;

    for id in &provider_ids {
        let result = registry.with_provider(id, |p| p.install_bridge_rule(payload));
        match result {
            Some(Ok(())) => {
                installed += 1;
            }
            Some(Err(e)) => {
                failed += 1;
                log::warn!(
                    "[sync-watcher] install_bridge_rule failed for provider '{}': {}",
                    id,
                    e
                );
            }
            None => {
                // Provider disappeared between provider_ids() and with_provider() —
                // benign race with concurrent deactivation.
            }
        }
    }

    log::info!(
        "[sync-watcher] bridge rule install: {}/{} providers OK (failed: {})",
        installed,
        provider_ids.len(),
        failed
    );
}

fn regenerate_agents_md(general_dir: &std::path::Path) {
    let agent_content = std::fs::read_to_string(general_dir.join("AGENT.md")).unwrap_or_default();
    let memory_content = std::fs::read_to_string(general_dir.join("MEMORY.md")).unwrap_or_default();

    let agents_content = format!(
        "{}\n\n---\n\n{}\n\n---\n\n{}\n\n---\n\n{}",
        agent_content,
        memory_content,
        crate::prompts::CODETERM_RULE,
        crate::prompts::BRIDGE_MEM,
    );

    if let Err(e) = std::fs::write(general_dir.join("AGENTS.md"), agents_content) {
        log::error!("[sync-watcher] Failed to write AGENTS.md: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_mem_payload_is_present_and_nonempty() {
        // Sanity check that the compiled-in payload is loaded.
        let p = crate::prompts::BRIDGE_MEM;
        assert!(!p.is_empty(), "BRIDGE_MEM should not be empty");
        assert!(
            p.contains("codeterm mem"),
            "BRIDGE_MEM should teach `codeterm mem`"
        );
        assert!(
            p.contains("codeterm mem search"),
            "BRIDGE_MEM should name the search subcommand"
        );
        assert!(
            p.contains("mem save"),
            "BRIDGE_MEM should name the save subcommand"
        );
    }

    #[test]
    fn regenerate_agents_md_writes_expected_sections() {
        let tmp = std::env::temp_dir().join("codeterm_sync_regen_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("AGENT.md"), "# Agent section").unwrap();
        std::fs::write(tmp.join("MEMORY.md"), "# Memory section").unwrap();

        regenerate_agents_md(&tmp);

        let written = std::fs::read_to_string(tmp.join("AGENTS.md")).unwrap();
        assert!(written.contains("# Agent section"));
        assert!(written.contains("# Memory section"));
        assert!(written.contains("CodeTerm Mem"), "bridge mem section missing");
        assert!(
            written.contains("codeterm mem search"),
            "bridge mem CLI reference missing"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn install_bridge_rules_empty_registry_is_harmless() {
        let reg = AgentRegistry::new();
        // No providers registered — should succeed silently (no panics, no writes).
        install_bridge_rules_for_all(&reg);
    }
}
