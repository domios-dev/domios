//! Immutable, type-keyed store for the long-tail services `ServiceContext` carries.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

type Slot = Box<dyn Any + Send + Sync>;

pub struct Extensions {
    map: HashMap<TypeId, Slot>,
}

impl Extensions {
    pub fn get<T: Any + Send + Sync>(&self) -> &T {
        self.try_get::<T>()
            .unwrap_or_else(|| panic!("extension {} is not registered", std::any::type_name::<T>()))
    }

    pub fn try_get<T: Any + Send + Sync>(&self) -> Option<&T> {
        self.map.get(&TypeId::of::<T>()).and_then(|slot| {
            slot.downcast_ref::<T>()
                .or_else(|| slot.downcast_ref::<OnceLock<T>>().and_then(OnceLock::get))
        })
    }

    pub fn get_or_init<T: Any + Send + Sync>(&self, init: impl FnOnce() -> T) -> &T {
        self.map
            .get(&TypeId::of::<T>())
            .and_then(|slot| slot.downcast_ref::<OnceLock<T>>())
            .unwrap_or_else(|| {
                panic!(
                    "late extension {} is not registered",
                    std::any::type_name::<T>()
                )
            })
            .get_or_init(init)
    }

    pub fn set<T: Any + Send + Sync>(&self, value: T) -> Result<(), T> {
        match self
            .map
            .get(&TypeId::of::<T>())
            .and_then(|slot| slot.downcast_ref::<OnceLock<T>>())
        {
            Some(slot) => slot.set(value),
            None => Err(value),
        }
    }
}

#[derive(Default)]
pub struct ExtensionsBuilder {
    map: HashMap<TypeId, Slot>,
}

impl ExtensionsBuilder {
    pub fn insert<T: Any + Send + Sync>(&mut self, value: T) {
        if self
            .map
            .insert(TypeId::of::<T>(), Box::new(value))
            .is_some()
        {
            panic!(
                "extension {} is registered twice",
                std::any::type_name::<T>()
            );
        }
    }

    pub fn insert_late<T: Any + Send + Sync>(&mut self) {
        if self
            .map
            .insert(TypeId::of::<T>(), Box::new(OnceLock::<T>::new()))
            .is_some()
        {
            panic!(
                "extension {} is registered twice",
                std::any::type_name::<T>()
            );
        }
    }

    pub(crate) fn insert_service_slots(&mut self) {
        self.insert_late::<crate::CronState>();
        self.insert_late::<crate::CronWake>();
        self.insert_late::<crate::ports::sentinel::SentinelPorts>();
        self.insert_late::<crate::session::SessionState>();
        self.insert_late::<crate::startup::StorageAuthority>();
        self.insert_late::<std::sync::Weak<crate::service::ServiceContext>>();
        self.insert_late::<crate::service::cron::OwnerPromptRequests>();
        self.insert_late::<crate::service::session_persist::SessionPersistenceWorker>();
    }

    pub fn finish(self) -> Extensions {
        for (type_id, name) in required() {
            if !self.map.contains_key(&type_id) {
                panic!("required extension {name} is missing");
            }
        }
        Extensions { map: self.map }
    }
}

fn key<T: Any>() -> (TypeId, &'static str) {
    (TypeId::of::<T>(), std::any::type_name::<T>())
}

/// The single key list both the daemon and `ServiceFixture` must satisfy.
fn required() -> Vec<(TypeId, &'static str)> {
    use crate::service::{
        account::AccountCell, analytics::AnalyticsCell, billing::BillingCell,
        receiver_target::LocalRecordingTargets, receiver_target::ReceiverTargets,
        task_files::LaunchTaskFiles, workspace_cwd::WorkspaceRelocationLock, BridgeOutboundRx,
        LoadedPlanProjects, MuxEmitPending, PlanStartupLoaded, PlanWatchers, PublishedTabStatuses,
        WorkerIdentityAllocator,
    };
    vec![
        key::<LaunchTaskFiles>(),
        key::<WorkspaceRelocationLock>(),
        key::<AccountCell>(),
        key::<BillingCell>(),
        key::<AnalyticsCell>(),
        key::<crate::ports::orchestration::InstalledOrchestration>(),
        key::<crate::ShellCapabilityState>(),
        key::<Arc<crate::service::protocol_recorder::ProtocolRecorder>>(),
        key::<Arc<crate::mem::supervisor::Supervisor>>(),
        key::<Arc<crate::plugin::activity::PluginActivityRegistry>>(),
        key::<Arc<crate::mem::worker_pool::WorkerPool>>(),
        key::<Arc<crate::service::process_tree::ProviderTreeWitnesses>>(),
        key::<BridgeOutboundRx>(),
        key::<Arc<crate::bridge::supervisor::BridgeSupervisor>>(),
        key::<Arc<codeterm_bridge::bridge::threads::ThreadService>>(),
        key::<crate::service::agent_restart::RestartTracker>(),
        key::<crate::delayed_send::DelayedSendQueue>(),
        key::<Arc<crate::secrets::SecretRegistry>>(),
        key::<Arc<crate::transcribe::TranscribeRegistry>>(),
        key::<Arc<codeterm_plugin::converter::ConverterRegistry>>(),
        key::<Arc<codeterm_plugin::graph_nodes::GraphNodeRegistry>>(),
        key::<Arc<codeterm_plugin::event_source::EventSourceRegistry>>(),
        key::<codeterm_plugin::graph_events::SubscriptionTable>(),
        key::<crate::glance::GlanceRegistryHandle>(),
        key::<crate::bubble::BubbleRegistryHandle>(),
        key::<crate::repowatch::RepoWatchRegistryHandle>(),
        key::<crate::view_host::ViewRegistryHandle>(),
        key::<Arc<crate::service::tab_transport_chat::TabReopenGuards>>(),
        key::<crate::chatpreprocessor::ChatPreprocessorRegistryHandle>(),
        key::<crate::agent_commands::AgentCommandRegistryHandle>(),
        key::<codeterm_plugin::WebhookRegistryHandle>(),
        key::<codeterm_plugin::local_recording::LocalRecordingRegistryHandle>(),
        key::<codeterm_plugin::tab_menu::TabMenuRegistryHandle>(),
        key::<codeterm_plugin::surfaces::SurfaceRegistryHandle>(),
        key::<Arc<crate::service::surface::SurfaceRuntimeRegistry>>(),
        key::<Arc<crate::service::surface::SurfaceActivityGate>>(),
        key::<ReceiverTargets>(),
        key::<Arc<crate::service::local_recording::RecordingSelectionStore>>(),
        key::<LocalRecordingTargets>(),
        key::<MuxEmitPending>(),
        key::<PublishedTabStatuses>(),
        key::<Arc<crate::service::PublishedMuxState>>(),
        key::<Arc<crate::service::MuxEmitStats>>(),
        key::<crate::service::output_stream_activity::OutputStreamActivityTx>(),
        key::<crate::pasteboard::PasteboardState>(),
        key::<Arc<crate::service::input_parser::InputParser>>(),
        key::<crate::usage::TabActivity>(),
        key::<Arc<crate::user_last_activity::UserLastActivity>>(),
        key::<crate::usage::poller::UsagePollerSlot>(),
        key::<PlanWatchers>(),
        key::<LoadedPlanProjects>(),
        key::<PlanStartupLoaded>(),
        key::<Arc<crate::service::hierarchy::heartbeat_gate::Gate>>(),
        key::<Arc<crate::service::hierarchy::stalled_team::StallEpisodeTracker>>(),
        key::<Arc<crate::service::reply_relay::ReplyRelayRegistry>>(),
        key::<WorkerIdentityAllocator>(),
    ]
}

#[cfg(test)]
mod tests;
