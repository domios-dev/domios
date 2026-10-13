use super::*;

pub(super) fn describe<P: AgentProvider + ?Sized>(provider: &P) -> ProviderDescriptor {
    let platform = current_platform_tag();
    let mut settings = provider.settings_schema();
    if provider.structured_control().is_some() {
        settings.push(SettingField::Section {
            title: "Structured control".into(),
            description: None,
            fields: vec![SettingField::Toggle {
                key: "structured_control_enabled".into(),
                label: "Enable structured control".into(),
                default: true,
                description: None,
            }],
        });
    }
    if provider.usage_parser().is_some() {
        let probe_cmd = provider.usage_probe_command().unwrap_or("/usage");
        settings.push(usage_tracking_section(probe_cmd));
    } else if provider.reports_group_usage() {
        settings.push(usage_store_tracking_section());
    }
    ProviderDescriptor {
        agent_runtime: provider.agent_runtime(),
        supports_session_fork: provider.fork_session_args().is_some(),
        structured_control: provider.structured_control(),
        schema_version: PROVIDER_DESCRIPTOR_SCHEMA_VERSION,
        cli_status: None,
        id: provider.id().to_string(),
        catalogue_version: crate::model_catalog::current_version(provider.id()),
        display_name: provider.display_name().to_string(),
        icon_hint: provider.icon_hint().map(|s| s.to_string()),
        icon_svg: provider.icon_svg().map(|s| s.to_string()),
        icon_color: provider.icon_color().map(|s| s.to_string()),
        // Keep unavailable entries: `available` greys a model out in the
        // picker, it must not vanish it. Matches chatbackend describe().
        available_models: provider.available_models(),
        model_catalogue: provider.model_catalogue_capability(),
        model_groups: provider.model_groups(),
        default_model: provider.default_model(),
        reasoning_efforts: provider.reasoning_efforts(),
        default_reasoning_effort: provider.default_reasoning_effort(),
        settings_schema: settings,
        notifications_schema: provider.notifications_schema(),
        supports_lazy_skills: provider.supports_lazy_skills(),
        has_hooks: provider.has_hooks(),
        supports_model_endpoints: provider.model_provider_config().is_some(),
        supports_permission_bypass: !provider.auto_approve_flags().is_empty(),
        cli_binary: provider.cli_binary().map(|s| s.to_string()),
        install_command: provider.install_command(platform),
        badge: provider.badge().map(|s| s.to_string()),
        update_command: provider.update_command(platform),
        model_switch: provider.model_switch(),
        fresh_context_command: provider.fresh_context_command().map(|s| s.to_string()),
        compact_command: provider.compact_command().map(|s| s.to_string()),
        integration_trust_grant: provider.integration_trust_grant_capability(),
        supports_stop: provider.stop_keys().is_some(),
        supports_chat_ui: provider.supports_chat_ui(),
        supports_structured_prompts: provider.supports_structured_prompts(),
        supports_prompt_answers: provider.supports_prompt_answers(),
        supports_permission_auto_confirm: provider.supports_permission_auto_confirm(),
        supports_watcher: false,
        paste_limit_bytes: provider.paste_limit_bytes(),
        trust: provider.trust(),
        transport: if provider.agent_runtime().is_some() {
            ProviderTransport::ChatBackend
        } else {
            ProviderTransport::Pty
        },
        behavior_config: provider.behavior_config(),
        presets: Vec::new(),
        system_prompt_delivery: provider.system_prompt_delivery(),
        context_owner: ContextOwner::Plugin,
        availability: ProviderAvailability::default(),
    }
}
