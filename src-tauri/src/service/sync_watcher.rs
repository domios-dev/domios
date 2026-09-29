//! Installs CodeTerm's routing and memory rules on every registered provider at
//! startup and on plugin changes. The old MEMORY.md watch loop is gone —
//! persistent memory lives in `codeterm mem` (SQLite), not on disk.

use std::path::PathBuf;
use std::sync::Arc;

use crate::agent::registry::AgentRegistry;

pub fn start(_codeterm_dir: PathBuf, agent_registry: Arc<AgentRegistry>) {
    install_bridge_rules_for_all(&agent_registry);
}

/// Install the complete CodeTerm host rule on every registered provider.
/// Idempotent — safe to call on startup and on every plugin activation.
pub fn install_bridge_rules_for_all(registry: &AgentRegistry) {
    let payload = format!(
        "{}\n\n{}",
        crate::prompts::CODETERM_RULE,
        crate::prompts::BRIDGE_MEM
    );
    let provider_ids = registry.provider_ids();
    let mut installed = 0usize;
    let mut failed = 0usize;

    for id in &provider_ids {
        let result = registry.with_provider(id, |p| p.install_bridge_rule(&payload));
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
                // Provider disappeared between provider_ids() and with_provider() — benign race with concurrent deactivation.
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
    fn provider_rule_payloads_are_present_and_nonempty() {
        let routing = crate::prompts::CODETERM_RULE;
        let memory = crate::prompts::BRIDGE_MEM;
        assert!(
            !routing.trim().is_empty(),
            "CODETERM_RULE should not be empty"
        );
        assert!(!memory.trim().is_empty(), "BRIDGE_MEM should not be empty");
    }

    #[test]
    fn install_bridge_rules_empty_registry_is_harmless() {
        let reg = AgentRegistry::new();
        // No providers registered — should succeed silently (no panics, no writes).
        install_bridge_rules_for_all(&reg);
    }
}
