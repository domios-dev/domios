//! Shared provider+model fallback resolution for memory background workers
//! (extraction, dreaming REM, recluster, cron-fired mem jobs). Consolidates
//! what used to be 7 hand-copied ladders across `mem_host.rs`,
//! `socket/server/mem/mod.rs`, `api/mem_routes/{workers,dream}.rs`,
//! `cron/scheduler.rs`, `service/cron.rs`, and `dreaming/schedule.rs`.
//!
//! Lives here (not in the binary crate) so `dreaming/schedule.rs`, which is
//! part of this crate, can share it too — the binary crate's model catalog
//! is reached only through [`BackgroundModelPort`], mirroring the existing
//! `ports.rs` app-supplied-port pattern.

/// App-supplied: resolve a configured model string against a provider's
/// model catalog.
pub trait BackgroundModelPort: Send + Sync {
    /// Returns `(resolved_model, fell_back)`.
    ///
    /// `resolved_model == ""` means "let the provider's own default apply
    /// downstream" — either nothing was configured, or what was configured
    /// didn't match the provider's catalog. `fell_back` is `true` only in
    /// the second case (a specific model was requested but not honored),
    /// never for a simply-empty request.
    fn resolve(&self, provider_id: &str, configured: &str) -> (String, bool);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedWorkerAgent {
    pub agent: String,
    pub model: String,
    pub fell_back: bool,
}

/// Step 1 of the ladder: pick the agent/provider. An explicit override (e.g.
/// a caller-supplied `agent` param) wins outright; otherwise the per-worker
/// config field if set, else the global default agent.
pub fn resolve_worker_agent(
    explicit: Option<&str>,
    configured: &str,
    default_agent: &str,
) -> String {
    if let Some(explicit) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        return explicit.to_string();
    }
    if configured.is_empty() {
        default_agent.to_string()
    } else {
        configured.to_string()
    }
}

/// Steps 2+3 of the ladder: pick the raw model (the per-worker config field,
/// else the shared `worker_model` fallback) then resolve it against the
/// already-chosen provider's catalog via `compat`.
pub fn resolve_worker_model(
    configured: &str,
    worker_model_fallback: &str,
    provider_id: &str,
    compat: &dyn BackgroundModelPort,
) -> (String, bool) {
    let raw = if configured.is_empty() {
        worker_model_fallback
    } else {
        configured
    };
    compat.resolve(provider_id, raw)
}

/// Full ladder in one call — the common case (mem_host.rs, socket/api
/// worker-spawn call sites).
pub fn resolve_worker(
    explicit_agent: Option<&str>,
    configured_agent: &str,
    configured_model: &str,
    default_agent: &str,
    worker_model_fallback: &str,
    compat: &dyn BackgroundModelPort,
) -> ResolvedWorkerAgent {
    let agent = resolve_worker_agent(explicit_agent, configured_agent, default_agent);
    let (model, fell_back) =
        resolve_worker_model(configured_model, worker_model_fallback, &agent, compat);
    ResolvedWorkerAgent {
        agent,
        model,
        fell_back,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeCompat;
    impl BackgroundModelPort for FakeCompat {
        fn resolve(&self, provider_id: &str, configured: &str) -> (String, bool) {
            if configured.is_empty() {
                return (String::new(), false);
            }
            match (provider_id, configured) {
                ("claude", "sonnet") => ("sonnet".into(), false),
                ("codex", "gpt-5.5") => ("gpt-5.5".into(), false),
                // any other provider/model combo doesn't match — falls back
                _ => (String::new(), true),
            }
        }
    }

    #[test]
    fn agent_ladder_prefers_explicit_then_configured_then_default() {
        assert_eq!(
            resolve_worker_agent(Some("cursor"), "codex", "claude"),
            "cursor"
        );
        assert_eq!(resolve_worker_agent(None, "codex", "claude"), "codex");
        assert_eq!(resolve_worker_agent(None, "", "claude"), "claude");
        assert_eq!(resolve_worker_agent(Some(""), "", "claude"), "claude");
    }

    #[test]
    fn model_ladder_prefers_configured_then_worker_model_fallback() {
        let (model, fell_back) = resolve_worker_model("sonnet", "haiku", "claude", &FakeCompat);
        assert_eq!(model, "sonnet");
        assert!(!fell_back);

        let (model, fell_back) = resolve_worker_model("", "gpt-5.5", "codex", &FakeCompat);
        assert_eq!(model, "gpt-5.5");
        assert!(!fell_back);
    }

    #[test]
    fn incompatible_model_falls_back_and_reports_fell_back() {
        let (model, fell_back) = resolve_worker_model("gpt-5.5", "", "claude", &FakeCompat);
        assert_eq!(model, "");
        assert!(fell_back);
    }

    #[test]
    fn empty_configured_and_empty_fallback_is_not_a_fallback() {
        let (model, fell_back) = resolve_worker_model("", "", "claude", &FakeCompat);
        assert_eq!(model, "");
        assert!(!fell_back);
    }

    #[test]
    fn resolve_worker_composes_agent_and_model_ladders() {
        let resolved = resolve_worker(None, "", "", "claude", "sonnet", &FakeCompat);
        assert_eq!(
            resolved,
            ResolvedWorkerAgent {
                agent: "claude".into(),
                model: "sonnet".into(),
                fell_back: false,
            }
        );

        let resolved = resolve_worker(
            Some("cursor"),
            "codex",
            "gpt-5.5",
            "claude",
            "haiku",
            &FakeCompat,
        );
        assert_eq!(resolved.agent, "cursor");
        assert_eq!(resolved.model, "");
        assert!(resolved.fell_back);
    }
}
