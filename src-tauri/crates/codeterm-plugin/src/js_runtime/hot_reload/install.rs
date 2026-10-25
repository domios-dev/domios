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
            match (result, prepared) {
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
                (Err(error), _) => Err(error),
            }
        };
        self.flush_settings_changed();
        self.notify_plugin_index_changed();
        result
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
