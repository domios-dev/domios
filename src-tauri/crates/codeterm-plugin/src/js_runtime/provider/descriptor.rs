use super::*;
use crate::provider::ProviderDescriptor;

pub(super) struct DescriptorSnapshot {
    descriptor: ProviderDescriptor,
    manifest_settings_count: usize,
}

impl JsProvider {
    pub(super) fn refresh_descriptor(&mut self) {
        self.descriptor_snapshot = Some(DescriptorSnapshot {
            descriptor: crate::provider::descriptor::describe(self),
            manifest_settings_count: manifest_settings_sections(&self.manifest).len(),
        });
    }

    pub(super) fn cached_descriptor(&self) -> ProviderDescriptor {
        let Some(snapshot) = &self.descriptor_snapshot else {
            return crate::provider::descriptor::describe(self);
        };
        let mut descriptor = snapshot.descriptor.clone();
        descriptor.catalogue_version = crate::model_catalog::current_version(self.id());
        descriptor.available_models = self.available_models();
        descriptor.default_model = self.default_model();
        descriptor.reasoning_efforts = self.reasoning_efforts();
        descriptor.default_reasoning_effort = self.default_reasoning_effort();
        if !self.has_custom_settings_schema {
            descriptor.settings_schema = manifest_settings_sections(&self.manifest);
            descriptor.settings_schema.extend(
                snapshot.descriptor.settings_schema[snapshot.manifest_settings_count..]
                    .iter()
                    .cloned(),
            );
        }
        descriptor
    }
}
