macro_rules! agent_provider_integration {
    () => {
        /// Install a plugin-contributed skill file for this agent. Providers
        /// that can't carry skills (system-prompt-only agents) ignore this.
        fn install_skills(&self, _skills: &[crate::plugin_manager::SkillFile]) -> Result<(), String> {
            Ok(())
        }

        /// Remove previously installed skills owned by the given plugin ids.
        fn uninstall_skills(&self, _plugin_ids: &[&str]) -> Result<(), String> {
            Ok(())
        }

        /// Whether this provider can lazy-load skills via trigger words
        /// (Claude Skill tool). Agents without lazy loading fall back to
        /// merging skills into the system prompt up-front.
        fn supports_lazy_skills(&self) -> bool {
            false
        }

        /// Install the mem bridge payload in whatever slot this provider
        /// reads at session start. MUST NOT write into user-owned skill dirs
        /// (`~/.claude/skills/`, `~/.agents/skills/`).
        fn install_bridge_rule(&self, _payload: &str) -> Result<(), String> {
            Ok(())
        }

        fn uninstall_bridge_rule(&self) -> Result<(), String> {
            Ok(())
        }

        // Providers can integrate through hook configuration, managed settings, or marked files.

        /// Status of CodeTerm's notification integration with this provider —
        /// returned as opaque JSON so each provider shapes it for its own UI.
        /// Default: providers without external integration report `status: "ok"`
        /// and the Badge renders "available".
        fn check_integration(&self) -> serde_json::Value {
            serde_json::json!({ "status": "ok" })
        }

        /// The integration surface this provider owns and may reconcile.
        fn integration_kind(&self) -> IntegrationKind {
            IntegrationKind::None
        }

        fn permission_hook_capability(&self) -> Option<PermissionHookCapability> {
            None
        }

        fn parse_context_usage(
            &self,
            _record: &serde_json::Value,
        ) -> Option<codeterm_types::agent::context::TurnContext> {
            None
        }

        /// Startup repair is capability-gated so core never identifies providers.
        fn has_installable_integration(&self) -> bool {
            self.integration_kind() != IntegrationKind::None
        }

        /// Install CodeTerm's provider integration. `api_port` targets local callback
        /// routes when the integration uses hooks. Default no-op.
        fn install_integration(
            &self,
            _api_port: u16,
            options: &IntegrationInstallOptions,
        ) -> Result<(), String> {
            if !options.consent {
                return Err(
                    "provider integration installation requires explicit local-user consent".into(),
                );
            }
            Ok(())
        }

        fn uninstall_integration(&self) -> Result<(), String> {
            Ok(())
        }

        fn restore_integration(&self) -> Result<(), String> {
            Err("provider integration has no recoverable backup".into())
        }

        /// Optional `integration.trust_grant` declaration; default `None` — the
        /// provider's hooks need no trust grant.
        fn integration_trust_grant_capability(&self) -> Option<IntegrationTrustGrantCapability> {
            None
        }

        /// Provider-owned hook-trust grant scoped to the declared managed key(s).
        /// The provider CLI is the only hash authority; failure degrades honestly.
        fn grant_integration_trust(&self, _managed_keys: &[String]) -> serde_json::Value {
            serde_json::json!({ "granted": false, "degraded": true, "reason": "provider declares no trust grant capability" })
        }

        /// Retained for provider-owned read-only startup checks. Core owns consented
        /// contract repair so it can preserve consent and avoid trust re-grants.
        fn ensure_integration_on_startup(&self, _api_port: u16) {}

        // Project rules: only Claude reads `~/.claude/rules/domios.md`.
        // Others inject rules elsewhere, so installed defaults to true when file is absent.

        fn check_rule_status(&self) -> serde_json::Value {
            serde_json::json!({ "installed": true, "update_available": false })
        }

        fn install_rule(&self) -> Result<(), String> {
            Ok(())
        }

        // Self-presentation: frontend reads provider tabs, models, and settings purely from these + `describe()`. Defaults stay empty.
    };
}
