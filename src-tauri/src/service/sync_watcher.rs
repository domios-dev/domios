//! Installs the mem bridge rule on every registered agent provider at
//! startup and on plugin changes. The old MEMORY.md watch loop is gone —
//! persistent memory lives in `codeterm mem` (SQLite), not on disk.
//! See `docs/plans/mem-architecture.md` Phase 3.0.

use std::path::PathBuf;
use std::sync::Arc;

use crate::agent::registry::AgentRegistry;

pub fn start(_codeterm_dir: PathBuf, agent_registry: Arc<AgentRegistry>) {
    install_bridge_rules_for_all(&agent_registry);
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
    fn install_bridge_rules_empty_registry_is_harmless() {
        let reg = AgentRegistry::new();
        // No providers registered — should succeed silently (no panics, no writes).
        install_bridge_rules_for_all(&reg);
    }
}
