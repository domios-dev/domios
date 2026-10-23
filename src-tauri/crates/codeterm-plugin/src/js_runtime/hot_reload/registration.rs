use super::*;

impl HotReloadRegistry {
    pub(super) fn register_loaded(
        &self,
        manifest_path: &std::path::Path,
        manifest_mtime: SystemTime,
        entry_mtime: SystemTime,
        config_mtime: SystemTime,
        mut provider: super::super::JsProvider,
    ) -> Option<String> {
        let id = provider.manifest().id.clone();
        let version = provider.manifest().version.clone();
        self.load_errors
            .lock()
            .retain(|key, error| key != &id && error.path != manifest_path);
        let had_chatbackend = self.caps.chatbackend.contains(&id);
        self.register_vm(&id, provider.vm());
        let activated_at_startup = self.activated_startup.lock().contains(&id);
        if provider.manifest().requires_reload && !activated_at_startup {
            self.pending_reload.lock().insert(id.clone());
            self.state.lock().insert(
                id.clone(),
                Tracked {
                    manifest_path: manifest_path.to_path_buf(),
                    manifest_mtime,
                    entry_mtime,
                    config_mtime,
                    grants_mtime: grants_mtime_for(manifest_path),
                },
            );
            log::info!(
                "[plugin-watcher] {id} declares requiresReload — pending until daemon reload"
            );
            self.notify_plugin_index_changed();
            return Some(id);
        }
        self.bind_service(provider.vm());
        if let Some(api_port) =
            runtime_api_port().filter(|_| crate::global_writes::machine_global_write_allowed())
        {
            provider.ensure_integration_on_startup(api_port);
        }
        let had_tab_menu = self.caps.tab_menu.contains(&id);
        let had_agent_commands = self.caps.agent_command.contains(&id);
        let core = super::super::loader::is_core_plugin(provider.manifest(), provider.trust());
        self.caps
            .register_plugin_with_core(provider.manifest(), provider.vm(), core);
        self.notify_event_sources_changed();
        if had_tab_menu || self.caps.tab_menu.contains(&id) {
            self.emit_tab_menu_changed();
        }
        if had_agent_commands || self.caps.agent_command.contains(&id) {
            self.emit_agent_commands_changed();
        }
        if provider.manifest().capabilities.view {
            self.emit_plugin_view_reloaded(&id);
        }
        if provider.manifest().capabilities.agent_provider {
            provider.set_registrar(self.registrar.clone());
            self.registrar.register_provider(Box::new(provider));
            log::info!("[plugin-watcher] registered {id}");
            self.registrar.provider_reloaded(&id);
            self.emit_agent_provider_changed();
        } else {
            if had_chatbackend || provider.manifest().capabilities.chat_backend.enabled() {
                self.emit_agent_provider_changed();
            }
            log::info!(
                "[plugin-watcher] {id} capability registered live; not an agent provider — skipped agent registry"
            );
        }
        self.state.lock().insert(
            id.clone(),
            Tracked {
                manifest_path: manifest_path.to_path_buf(),
                manifest_mtime,
                entry_mtime,
                config_mtime,
                grants_mtime: grants_mtime_for(manifest_path),
            },
        );
        crate::audit::append_daemon_reloaded(&id, &version);
        self.notify_plugin_index_changed();
        #[cfg(test)]
        self.load_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Some(id)
    }
}
