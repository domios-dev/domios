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
        let started = std::time::Instant::now();
        let mut descriptor = snapshot.descriptor.clone();
        let clone_us = started.elapsed().as_micros();
        let catalogue_started = std::time::Instant::now();
        descriptor.catalogue_version = crate::model_catalog::current_version(self.id());
        descriptor.available_models = self.available_models();
        descriptor.default_model = self.default_model();
        descriptor.reasoning_efforts = self.reasoning_efforts();
        descriptor.default_reasoning_effort = self.default_reasoning_effort();
        let catalogue_us = catalogue_started.elapsed().as_micros();
        let settings_started = std::time::Instant::now();
        if !self.has_custom_settings_schema {
            descriptor.settings_schema = manifest_settings_sections(&self.manifest);
            descriptor.settings_schema.extend(
                snapshot.descriptor.settings_schema[snapshot.manifest_settings_count..]
                    .iter()
                    .cloned(),
            );
        }
        log::debug!("[providers.descriptor] provider={} clone_us={} catalogue_us={} settings_us={} total_us={}", self.id(), clone_us, catalogue_us, settings_started.elapsed().as_micros(), started.elapsed().as_micros());
        descriptor
    }
}
