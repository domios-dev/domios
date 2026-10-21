macro_rules! agent_provider_catalogue_head {
    () => {
        /// Models this provider offers (e.g. sonnet, opus, haiku for Claude).
        fn available_models(&self) -> Vec<ModelInfo> {
            Vec::new()
        }

        fn model_catalogue_capability(&self) -> ModelCatalogueCapability {
            ModelCatalogueCapability::default()
        }
    };
}

macro_rules! agent_provider_catalogue_tail {
    () => {
        /// Default model id, must match one of `available_models()` if non-None.
        /// Returns `String` (not `&str`) so providers can resolve through the
        /// user-editable `model_catalog` without lifetime headaches.
        fn default_model(&self) -> Option<String> {
            None
        }

        /// Provider-owned groupings of its models, declared as pure data (no I/O):
        /// the picker renders one section per group and core carries the labels
        /// through untouched. Empty ⇒ one flat list.
        fn model_groups(&self) -> Vec<ModelGroup> {
            Vec::new()
        }

        /// Cumulative usage per model group, from whatever source the provider
        /// declares. May do I/O, so it is never called from `describe()` — only
        /// from the on-demand picker command. Empty ⇒ the provider publishes no
        /// per-group usage and the picker shows none.
        fn model_group_usage(&self) -> Vec<ModelGroupUsage> {
            Vec::new()
        }

        /// Cheap, pure predicate: this provider publishes per-group usage even
        /// without a usage parser. It drives the auto-injected Usage Tracking
        /// toggle, so a group-spend capsule is gated like any other usage surface.
        fn reports_group_usage(&self) -> bool {
            false
        }

        /// Files whose change means this provider's credentialed model availability
        /// changed. The catalog fingerprints them and re-probes when one moves, so
        /// connecting a provider surfaces its models without a rebuild. Empty ⇒ the
        /// provider's catalog does not depend on an observable credential file.
        fn model_credential_files(&self) -> Vec<std::path::PathBuf> {
            Vec::new()
        }

        fn icon_hint(&self) -> Option<&str> {
            None
        }

        fn icon_svg(&self) -> Option<&str> {
            None
        }

        fn icon_color(&self) -> Option<&str> {
            None
        }

        /// Name of the binary the provider's launch commands invoke (e.g. `claude`,
        /// `codex`, `opencode`). When `Some`, CodeTerm checks PATH availability so
        /// the Settings UI can render an "install" affordance for missing CLIs.
        /// Default `None` — provider opts out of installation tracking.
        fn cli_binary(&self) -> Option<&str> {
            None
        }

        /// Per-platform install command run inside a fresh terminal tab when the
        /// user clicks "Install" in Settings. `platform` is one of `"macos"`,
        /// `"linux"`, `"windows"`. Return `None` for platforms where you don't ship
        /// an installer recipe.
        fn install_command(&self, _platform: &str) -> Option<String> {
            None
        }

        fn update_command(&self, _platform: &str) -> Option<String> {
            None
        }

        fn provider_self_update_capability(
            &self,
        ) -> Option<crate::provider_self_update::ProviderSelfUpdateCapability> {
            None
        }

        fn supports_update_check(&self) -> bool {
            false
        }

        fn update_prompt_availability(&self, screen: &str) -> Option<ProviderUpdateAvailability> {
            super::update_prompt::availability(screen)
        }

        fn check_for_update(&self) -> Option<ProviderUpdateAvailability> {
            None
        }

        /// Short note picker surfaces show beside this provider's name, such as a
        /// free-tier offer. `None` means the provider makes no such claim.
        fn badge(&self) -> Option<&str> {
            None
        }

        /// Declarative settings panel for the per-provider Settings → Agents tab.
        /// Frontend renders generically via `<SchemaRenderer />`.
        fn settings_schema(&self) -> Vec<SettingField> {
            Vec::new()
        }

        /// Declarative schema contributed to the global Settings → Notifications
        /// tab. Each provider that wants to plug into notifications (e.g. Claude
        /// installing hook handlers) returns a non-empty schema; the renderer
        /// composes one section per contributing provider. Default empty.
        fn notifications_schema(&self) -> Vec<SettingField> {
            Vec::new()
        }

        /// Build the wire descriptor reported to the frontend.
        /// **Contract:** this method MUST be pure, cheap, and free of I/O.
        /// No locks, no `.await`, no remote calls. It is invoked while
        /// holding a read lock on the agent registry, and the result is
        /// serialized to JSON for REST/Socket/Tauri callers. The default
        /// impl just assembles the other metadata methods.
        fn describe(&self) -> ProviderDescriptor {
            super::descriptor::describe(self)
        }

        /// Behavior surface exposed for configuration. PTY agents bring their own
        /// prompt set by default.
        fn behavior_config(&self) -> BehaviorConfig {
            BehaviorConfig::OwnPromptSet
        }

        /// How a system prompt reaches this provider. Default `External` — PTY
        /// agents carry their own prompts and take no system-prompt flag here.
        fn system_prompt_delivery(&self) -> SystemPromptDelivery {
            SystemPromptDelivery::External
        }

        /// Whether a tab running this provider can be shown as chat. Defaults
        /// to "yes if it serves structured chat"; JSONL providers override to
        /// `true`. The frontend reads this off the descriptor — no hardcoded
        /// provider list.
        fn supports_chat_ui(&self) -> bool {
            self.uses_structured_chat()
        }

        fn supports_prompt_answers(&self) -> bool {
            true
        }

        /// Whether this provider exposes a structured source for pending prompts.
        fn supports_structured_prompts(&self) -> bool {
            false
        }

        /// Whether a dedicated directory-trust parser may run after a cheap screen precheck.
        fn supports_directory_trust_prompt_parsing(&self) -> bool {
            false
        }

        /// Whether `permission_prompt_response` can recognize any prompt, so the UI
        /// offers per-tab auto-confirm only where it can ever answer.
        fn supports_permission_auto_confirm(&self) -> bool {
            false
        }

        fn supports_classified_prompt_auto_response(&self) -> bool {
            false
        }

        fn classified_prompt_auto_response(
            &self,
            _state: crate::tab_state::TabState,
            _screen: &str,
        ) -> Option<crate::provider::ClassifiedPromptOptionRole> {
            None
        }

        /// Pending prompt from this exact session; pure, I/O-only, capability gated.
        fn structured_pending_prompt(
            &self,
            _cwd: &str,
            _session_id: &str,
        ) -> Option<codeterm_types::pty::PendingPrompt> {
            None
        }

        /// Whether the host must bind a local server for structured prompts.
        fn needs_structured_prompt_server(&self) -> bool {
            false
        }

        /// Bundle trust status. Default impl returns `Unknown` (native
        /// providers — no hash to check). JS-backed providers override
        /// to return System/Unverified/ThirdParty based on the
        /// build-time hash table.
        fn trust(&self) -> ProviderTrust {
            ProviderTrust::Unknown
        }

        /// Whether this is a native (Rust) implementation. JS drop-ins override to
        /// `false`. The registry relies on this so it never records a JS provider as
        /// the "native fallback" another provider delegates to: a JS-only provider
        /// re-registered on hot-reload would otherwise become its own fallback, and
        /// `JsProvider::native()` returning itself makes capability delegation
        /// recurse until the stack overflows.
        fn is_native(&self) -> bool {
            true
        }
    };
}
