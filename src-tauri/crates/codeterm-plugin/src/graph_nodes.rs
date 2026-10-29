use crate::blueprint_step::{BlueprintStep, Estimate, JobUpdate, StepCall, StepDescriptor};
use crate::js_runtime::manifest::{EventSourceDecl, GraphNodesCap};
use parking_lot::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const MAX_GRAPH_NODE_PLUGINS: usize = 256;

#[derive(Default)]
pub struct GraphNodeRegistry {
    entries: RwLock<Vec<(String, Vec<EventSourceDecl>)>>,
    steps: RwLock<Vec<RegisteredStep>>,
}

#[derive(Clone)]
pub struct RegisteredStep {
    pub cost_units: crate::cost_units::CostUnits,
    pub plugin: String,
    pub descriptor: StepDescriptor,
    pub adapter: Arc<dyn BlueprintStep>,
    active: Arc<AtomicBool>,
}

struct RegisteredAdapter {
    active: Arc<AtomicBool>,
    adapter: Arc<dyn BlueprintStep>,
}

impl RegisteredAdapter {
    fn ensure_active(&self) -> Result<(), String> {
        self.active
            .load(Ordering::SeqCst)
            .then_some(())
            .ok_or_else(|| "plugin step removed".into())
    }
    fn call<T>(
        &self,
        call: impl FnOnce(&dyn BlueprintStep) -> Result<T, String>,
    ) -> Result<T, String> {
        self.ensure_active()?;
        let result = call(self.adapter.as_ref());
        self.ensure_active()?;
        result
    }
    fn job_call(
        &self,
        call: impl FnOnce(&dyn BlueprintStep) -> Result<JobUpdate, String>,
    ) -> Result<JobUpdate, String> {
        self.ensure_active()?;
        let result = call(self.adapter.as_ref());
        if self.ensure_active().is_err() {
            return result.map(|update| JobUpdate::unknown(update.job));
        }
        result
    }
}

impl BlueprintStep for RegisteredAdapter {
    fn estimate(&self, call: &StepCall<'_>) -> Result<Option<Estimate>, String> {
        self.call(|adapter| adapter.estimate(call))
    }
    fn submit(&self, call: &StepCall<'_>) -> Result<JobUpdate, String> {
        self.job_call(|adapter| adapter.submit(call))
    }
    fn reconcile(&self, call: &StepCall<'_>) -> Result<JobUpdate, String> {
        self.job_call(|adapter| adapter.reconcile(call))
    }
    fn cancel(&self, call: &StepCall<'_>) -> Result<JobUpdate, String> {
        self.job_call(|adapter| adapter.cancel(call))
    }
}

impl GraphNodeRegistry {
    pub fn register(&self, id: impl Into<String>, nodes: &GraphNodesCap) -> Result<(), String> {
        let id = id.into();
        let mut entries = self.entries.write();
        let existing = entries.iter().position(|(registered, _)| registered == &id);
        if existing.is_none() && entries.len() >= MAX_GRAPH_NODE_PLUGINS {
            return Err("graph node capability registry is full".into());
        }
        let value = (id, nodes.event_sources.clone());
        if let Some(index) = existing {
            entries[index] = value;
        } else {
            entries.push(value);
        }
        Ok(())
    }

    pub fn unregister(&self, id: &str) {
        self.remove_steps(id);
        self.entries
            .write()
            .retain(|(registered, _)| registered != id);
    }

    pub fn register_steps(
        &self,
        plugin: &str,
        descriptors: &[StepDescriptor],
        adapter: Arc<dyn BlueprintStep>,
    ) -> Result<(), String> {
        self.register_steps_with_units(plugin, descriptors, adapter, Default::default())
    }

    pub fn register_steps_with_units(
        &self,
        plugin: &str,
        descriptors: &[StepDescriptor],
        adapter: Arc<dyn BlueprintStep>,
        cost_units: crate::cost_units::CostUnits,
    ) -> Result<(), String> {
        if descriptors.len() > crate::blueprint_step::MAX_STEP_DESCRIPTORS {
            return Err("too many plugin steps".into());
        }
        let mut steps = self.steps.write();
        if steps.iter().filter(|step| step.plugin != plugin).count() + descriptors.len()
            > MAX_GRAPH_NODE_PLUGINS * crate::blueprint_step::MAX_STEP_DESCRIPTORS
        {
            return Err("plugin step registry is full".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        for descriptor in descriptors {
            descriptor.validate()?;
            if !descriptor.id.starts_with(&format!("{plugin}."))
                || !ids.insert(&descriptor.id)
                || steps
                    .iter()
                    .any(|s| s.plugin != plugin && s.descriptor.id == descriptor.id)
            {
                return Err("plugin step namespace collision".into());
            }
        }
        for step in steps.iter().filter(|step| step.plugin == plugin) {
            step.active.store(false, Ordering::SeqCst);
        }
        steps.retain(|step| step.plugin != plugin);
        let active = Arc::new(AtomicBool::new(true));
        let adapter: Arc<dyn BlueprintStep> = Arc::new(RegisteredAdapter {
            active: active.clone(),
            adapter,
        });
        steps.extend(descriptors.iter().map(|descriptor| RegisteredStep {
            cost_units: cost_units.clone(),
            plugin: plugin.into(),
            descriptor: descriptor.clone(),
            adapter: adapter.clone(),
            active: active.clone(),
        }));
        Ok(())
    }

    fn remove_steps(&self, plugin: &str) {
        let mut steps = self.steps.write();
        for step in steps.iter().filter(|step| step.plugin == plugin) {
            step.active.store(false, Ordering::SeqCst);
        }
        steps.retain(|step| step.plugin != plugin);
    }

    pub fn steps(&self) -> Vec<RegisteredStep> {
        self.steps.read().clone()
    }

    pub fn step(&self, id: &str) -> Option<RegisteredStep> {
        self.steps
            .read()
            .iter()
            .find(|s| s.descriptor.id == id)
            .cloned()
    }

    pub fn event_sources(&self) -> Vec<EventSourceDecl> {
        self.entries
            .read()
            .iter()
            .flat_map(|(_, sources)| sources.iter().cloned())
            .collect()
    }
}
