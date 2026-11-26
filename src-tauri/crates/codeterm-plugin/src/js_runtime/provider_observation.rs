use super::host_api::PluginSandbox;
use super::manifest::PluginManifest;
use crate::provider_version::{ProbeStatus, VersionObservation};

pub(super) struct ProviderObservation {
    pub(super) binary: Option<String>,
    pub(super) aliases: Vec<String>,
    pub(super) compatibility: Option<crate::provider_compat::CompatibilityManifest>,
    #[cfg(test)]
    fixture_version: Option<VersionObservation>,
}

impl ProviderObservation {
    pub(super) fn from_manifest(json: Option<&str>) -> Self {
        let manifest = json.and_then(|json| PluginManifest::from_json_str(json).ok());
        Self {
            #[cfg(test)]
            fixture_version: FIXTURE_VERSION.with(|fixture| fixture.borrow().clone()),
            binary: manifest
                .as_ref()
                .filter(|m| m.capabilities.agent_provider)
                .map(|m| {
                    m.cli_binary
                        .as_ref()
                        .or(m.binary_name.as_ref())
                        .unwrap_or(&m.id)
                        .clone()
                }),
            aliases: manifest
                .as_ref()
                .filter(|m| m.capabilities.agent_provider)
                .map(|m| m.cli_binary_aliases.clone())
                .unwrap_or_default(),
            compatibility: manifest.and_then(|m| m.cli_compatibility),
        }
    }

    pub(super) fn version(&self, sandbox: &PluginSandbox) -> VersionObservation {
        #[cfg(test)]
        if let Some(version) = &self.fixture_version {
            return version.clone();
        }
        let candidates: Vec<&str> = self
            .binary
            .iter()
            .chain(self.aliases.iter())
            .map(String::as_str)
            .filter(|binary| sandbox.can_exec(binary))
            .collect();
        if candidates.is_empty() {
            VersionObservation {
                version: None,
                status: ProbeStatus::Unreadable,
            }
        } else {
            crate::provider_version::observe_candidates_in(&candidates, sandbox.extra_bin_dirs())
        }
    }

    pub(super) fn sqlite_compatibility(
        &self,
        sandbox: &PluginSandbox,
    ) -> Option<crate::provider_compat::CompatibilityEvaluation> {
        self.compatibility
            .as_ref()
            .filter(|policy| policy.direct_sqlite)
            .map(|policy| {
                let version = self.version(sandbox);
                policy.evaluate(version.version.as_deref())
            })
    }
}

#[cfg(test)]
thread_local! {
    static FIXTURE_VERSION: std::cell::RefCell<Option<VersionObservation>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn with_fixture_version<T>(version: Option<&str>, run: impl FnOnce() -> T) -> T {
    struct Restore(Option<VersionObservation>);
    impl Drop for Restore {
        fn drop(&mut self) {
            FIXTURE_VERSION.with(|fixture| {
                fixture.replace(self.0.take());
            });
        }
    }
    let _restore = Restore(FIXTURE_VERSION.with(|fixture| {
        fixture.replace(Some(VersionObservation {
            version: version.map(str::to_owned),
            status: if version.is_some() {
                ProbeStatus::Detected
            } else {
                ProbeStatus::Failed
            },
        }))
    }));
    run()
}
