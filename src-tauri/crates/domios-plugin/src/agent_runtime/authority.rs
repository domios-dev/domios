use crate::host_traits::PluginHostCtx;
use crate::js_runtime::manifest::RuntimeManifestError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLaunchBinding {
    pub tab_id: String,
    pub provider_id: String,
    pub binding_nonce: String,
}

#[derive(Debug, Clone)]
pub struct LaunchAuthority {
    binding: RuntimeLaunchBinding,
}

impl LaunchAuthority {
    pub fn verify(
        host: &dyn PluginHostCtx,
        binding: RuntimeLaunchBinding,
    ) -> Result<Self, RuntimeManifestError> {
        if binding.tab_id.is_empty()
            || binding.provider_id.is_empty()
            || binding.binding_nonce.is_empty()
            || !host.runtime_launch_authorized(&binding)
        {
            return Err(RuntimeManifestError::ConsentRequired);
        }
        Ok(Self { binding })
    }

    pub fn binding(&self) -> &RuntimeLaunchBinding {
        &self.binding
    }

    pub fn authorizes_provider(&self, id: &str) -> bool {
        self.binding.provider_id == id
    }
}

#[cfg(test)]
mod tests;
