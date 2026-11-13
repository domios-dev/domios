use super::*;

pub fn with_quiesced_install(
    id: &str,
    destination: &std::path::Path,
    operation: impl FnOnce(&mut dyn FnMut() -> Result<(), String>) -> Result<(), String>,
) -> Result<(), String> {
    WATCHER
        .get()
        .ok_or("Plugin watcher is not ready")?
        .install(id, destination, operation)
}

impl HotReloadRegistry {
    pub fn install(
        &self,
        id: &str,
        destination: &std::path::Path,
        operation: impl FnOnce(&mut dyn FnMut() -> Result<(), String>) -> Result<(), String>,
    ) -> Result<(), String> {
        let result = {
            let _reconciliation = self.reconciliation.lock();
            let service = self
                .service
                .lock()
                .clone()
                .and_then(|service| service.upgrade());
            let _watches = service
                .as_ref()
                .map(|service| service.suspend_plugin_watches(destination))
                .transpose()?;
            let released = self
                .file_events
                .lock()
                .as_mut()
                .map(|events| events.release_bundle(destination))
                .transpose();
            if let Err(error) = released {
                self.restore_install_watches()?;
                return Err(format!("Release plugin directory watches: {error}"));
            }
            let access = crate::bundle_access::BundleAccess::for_plugin(id);
            let path = destination.join("plugin.json");
            let mut prepared = None;
            let result = access.install(|| {
                super::super::host_api::sqlite::drop_plugin_connections(id);
                super::super::host_api::release_bundle_file_locks(destination);
                operation(&mut || {
                    let provider = self.prepare_install(id, &path);
                    if provider.is_err() {
                        super::super::host_api::sqlite::drop_plugin_connections(id);
                        super::super::host_api::release_bundle_file_locks(destination);
                    }
                    let provider = provider?;
                    let entry = provider
                        .manifest()
                        .entry_path()
                        .ok_or("Plugin entry path is missing")?;
                    prepared = Some((provider, entry));
                    Ok(())
                })
            });
            let result = match (result, prepared) {
                (Ok(()), Some((provider, entry))) => {
                    self.failed.lock().remove(&path);
                    self.register_loaded(
                        &path,
                        mtime_or_epoch(&path),
                        mtime_or_epoch(&entry),
                        config_mtime_for(&path, id),
                        provider,
                    );
                    Ok(())
                }
                (Ok(()), None) => Err("Plugin swap did not prepare its registration".into()),
                (Err(error), _) => {
                    let tabs = service
                        .as_ref()
                        .map(|service| service.plugin_install_tabs(id))
                        .unwrap_or_default();
                    Err(install_failure(id, error, &tabs))
                }
            };
            match (result, self.restore_install_watches()) {
                (Err(error), Err(restore)) => Err(format!("{error}; {restore}")),
                (Err(error), _) | (_, Err(error)) => Err(error),
                (Ok(()), Ok(())) => Ok(()),
            }
        };
        self.flush_settings_changed();
        self.notify_plugin_index_changed();
        result
    }

    fn restore_install_watches(&self) -> Result<(), String> {
        self.file_events
            .lock()
            .as_mut()
            .map(|events| events.refresh())
            .transpose()
            .map(|_| ())
            .map_err(|error| format!("Restore plugin directory watches: {error}"))
    }

    fn prepare_install(
        &self,
        id: &str,
        path: &std::path::Path,
    ) -> Result<super::super::JsProvider, String> {
        let manifest = PluginManifest::load(path)?;
        if manifest.id != id {
            return Err(format!("Installed plugin identity differs from {id}"));
        }
        if manifest.requires_reload && !self.activated_startup.lock().contains(id) {
            return Err(format!("Plugin {id} requires a daemon reload"));
        }
        load_one(path)
    }
}

pub(super) fn install_failure(id: &str, error: String, tabs: &[String]) -> String {
    let sharing_failure = ["os error 5)", "os error 32)", "os error 33)"]
        .iter()
        .any(|code| error.contains(code));
    if cfg!(windows) && sharing_failure && !tabs.is_empty() {
        format!("Plugin {id} could not be replaced while provider tabs are open: {}. Close these tabs with `domios tab close --tab <id>` and install again. {error}", tabs.join(", "))
    } else {
        error
    }
}
