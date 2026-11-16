use crate::service::extensions::{Extensions, ExtensionsBuilder};
use crate::service::{
    messenger::MessageEnvelope,
    neighbor_state::{Mutation, NeighborAuthority},
    spawn::SpawnTabSpec,
    tab_credentials::TabCaller,
    ServiceState, SpawnAgentParams,
};
use codeterm_account::account_mesh::AccountMesh;
use codeterm_neighbors::permissions::Permission;
use codeterm_types::pty::TabStatus;
use codeterm_wire::api_contracts::State;
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, OnceLock},
};
pub type NeighborFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait NeighborsPort: Send + Sync {
    fn account<'a>(&'a self) -> NeighborFuture<'a, Result<Arc<codeterm_account::Account>, String>>;
    fn account_mesh<'a>(&'a self) -> NeighborFuture<'a, Result<Arc<AccountMesh>, String>>;
    fn authorize_neighbor_rpc_credential<'a>(
        &'a self,
        credential: &'a str,
        method: &'a str,
        params: Option<&'a serde_json::Value>,
    ) -> NeighborFuture<'a, Result<(), String>>;
    fn authorize_neighbor_secret_rpc(
        &self,
        caller: &TabCaller,
        method: &str,
        params: Option<&serde_json::Value>,
    ) -> Result<(), String>;
    fn carry_neighbor_restore_grant(
        &self,
        tab_id: &str,
        old_nonce: &str,
        new_nonce: &str,
    ) -> std::result::Result<(), String>;
    fn inherit_neighbor_agent_spawn<'a>(
        &'a self,
        params: &'a mut SpawnAgentParams,
    ) -> NeighborFuture<'a, Result<bool, String>>;
    fn inherit_neighbor_agent_spawn_sync(
        &self,
        params: &mut SpawnAgentParams,
    ) -> Result<bool, String>;
    fn neighbor_entitlements_changed(&self, changed: bool);
    fn neighbor_friend_display_name(&self, friendship_id: &str) -> Option<String>;
    fn neighbor_launch_caller(&self, callers: &[Option<&str>])
        -> Result<Option<TabCaller>, String>;
    fn neighbor_memory_areas<'a>(
        &'a self,
        caller: &'a TabCaller,
    ) -> NeighborFuture<'a, Result<Option<Vec<String>>, String>>;
    fn neighbor_memory_peer_uid<'a>(
        &'a self,
        authority: &'a NeighborAuthority,
    ) -> NeighborFuture<'a, Result<String, String>>;
    fn neighbor_memory_write_grant<'a>(
        &'a self,
        caller: &'a TabCaller,
    ) -> NeighborFuture<'a, Result<(NeighborAuthority, Permission), String>>;
    fn neighbor_runtime_handling_event<'a>(
        &'a self,
        tab: &'a str,
        nonce: &'a str,
        thread: &'a str,
        turn: &'a str,
        message: Option<&'a str>,
        state: State,
    ) -> NeighborFuture<'a, codeterm_neighbors::Result<()>>;
    fn neighbor_scoped_tool_launch_args(
        &self,
        tab_id: &str,
        provider: &str,
        params: &SpawnAgentParams,
    ) -> std::result::Result<Vec<String>, String>;
    fn neighbor_spawn_friend_name(&self, target: &str) -> Option<String>;
    fn observe_neighbor_pty_handling(&self, tab: &str, status: TabStatus);
    fn prepare_neighbor_restore_grant(
        &self,
        tab_id: &str,
        provider: &str,
        params: &SpawnAgentParams,
    ) -> std::result::Result<(), String>;
    fn prepare_neighbor_root_close(&self, tab: &str) -> Result<Option<Mutation>, String>;
    fn prepare_neighbor_tab_spawn(&self, spec: &mut SpawnTabSpec) -> Result<(), String>;
    fn require_neighbor_restore_grant(
        &self,
        tab_id: &str,
        bypass: bool,
    ) -> std::result::Result<(), String>;
    fn session_revocations(
        &self,
    ) -> Arc<dyn codeterm_account::session_revocation::SessionRevocationObserver>;
    fn stamp_neighbor_message(&self, envelope: MessageEnvelope) -> MessageEnvelope;
    fn unlock_account_mesh_after_plan_change(&self);
}
pub type Factory = fn(ServiceState) -> Arc<dyn NeighborsPort>;
pub type ExtensionRegistrar = fn(&mut ExtensionsBuilder);

static FACTORY: OnceLock<(Factory, ExtensionRegistrar)> = OnceLock::new();

/// Per-context slot, filled once right after the context is wrapped in its `Arc`.
#[derive(Default)]
pub(crate) struct InstalledNeighbors(OnceLock<Arc<dyn NeighborsPort>>);

pub fn register_neighbors(make: Factory, extensions: ExtensionRegistrar) {
    let _ = FACTORY.set((make, extensions));
}

pub fn register_extensions(builder: &mut ExtensionsBuilder) {
    let (_, extensions) = FACTORY
        .get()
        .expect("neighbors port is registered before the service starts");
    extensions(builder);
}

pub fn install(svc: &ServiceState) {
    let (make, _) = FACTORY
        .get()
        .expect("neighbors port is registered before the service starts");
    fill(svc, make(svc.clone()));
}

fn fill(svc: &ServiceState, port: Arc<dyn NeighborsPort>) {
    let _ = svc.extensions.get::<InstalledNeighbors>().0.set(port);
}

pub(crate) fn installed(extensions: &Extensions) -> &Arc<dyn NeighborsPort> {
    extensions
        .get::<InstalledNeighbors>()
        .0
        .get()
        .expect("neighbors port is installed after ServiceContext construction")
}

/// Fixtures get the registered crate when a test registered it, else the recording fake.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn register_fixture_extensions(builder: &mut ExtensionsBuilder) {
    if let Some((_, extensions)) = FACTORY.get() {
        extensions(builder);
    }
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn install_fixture(svc: &ServiceState) {
    match FACTORY.get() {
        Some((make, _)) => fill(svc, make(svc.clone())),
        None => fill(svc, Arc::new(RecordingNeighbors::default())),
    }
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
pub struct RecordingNeighbors {
    pub calls: parking_lot::Mutex<Vec<&'static str>>,
}
#[cfg(any(test, feature = "test-support"))]
impl NeighborsPort for RecordingNeighbors {
    fn account<'a>(&'a self) -> NeighborFuture<'a, Result<Arc<codeterm_account::Account>, String>> {
        self.calls.lock().push("account");
        Box::pin(async { Err("neighbors recording fixture has no configured response".into()) })
    }
    fn account_mesh<'a>(&'a self) -> NeighborFuture<'a, Result<Arc<AccountMesh>, String>> {
        self.calls.lock().push("account_mesh");
        Box::pin(async { Err("neighbors recording fixture has no configured response".into()) })
    }
    fn authorize_neighbor_rpc_credential<'a>(
        &'a self,
        credential: &'a str,
        method: &'a str,
        params: Option<&'a serde_json::Value>,
    ) -> NeighborFuture<'a, Result<(), String>> {
        self.calls.lock().push("authorize_neighbor_rpc_credential");
        let _ = credential;
        let _ = method;
        let _ = params;
        Box::pin(async { Ok(()) })
    }
    fn authorize_neighbor_secret_rpc(
        &self,
        caller: &TabCaller,
        method: &str,
        params: Option<&serde_json::Value>,
    ) -> Result<(), String> {
        self.calls.lock().push("authorize_neighbor_secret_rpc");
        let _ = caller;
        let _ = method;
        let _ = params;
        Ok(())
    }
    fn carry_neighbor_restore_grant(
        &self,
        tab_id: &str,
        old_nonce: &str,
        new_nonce: &str,
    ) -> std::result::Result<(), String> {
        self.calls.lock().push("carry_neighbor_restore_grant");
        let _ = tab_id;
        let _ = old_nonce;
        let _ = new_nonce;
        Ok(())
    }
    fn inherit_neighbor_agent_spawn<'a>(
        &'a self,
        params: &'a mut SpawnAgentParams,
    ) -> NeighborFuture<'a, Result<bool, String>> {
        self.calls.lock().push("inherit_neighbor_agent_spawn");
        let _ = params;
        Box::pin(async { Ok(false) })
    }
    fn inherit_neighbor_agent_spawn_sync(
        &self,
        params: &mut SpawnAgentParams,
    ) -> Result<bool, String> {
        self.calls.lock().push("inherit_neighbor_agent_spawn_sync");
        let _ = params;
        Ok(false)
    }
    fn neighbor_entitlements_changed(&self, changed: bool) {
        self.calls.lock().push("neighbor_entitlements_changed");
        let _ = changed;
    }
    fn neighbor_friend_display_name(&self, friendship_id: &str) -> Option<String> {
        self.calls.lock().push("neighbor_friend_display_name");
        let _ = friendship_id;
        None
    }
    fn neighbor_launch_caller(
        &self,
        callers: &[Option<&str>],
    ) -> Result<Option<TabCaller>, String> {
        self.calls.lock().push("neighbor_launch_caller");
        let _ = callers;
        Ok(None)
    }
    fn neighbor_memory_areas<'a>(
        &'a self,
        caller: &'a TabCaller,
    ) -> NeighborFuture<'a, Result<Option<Vec<String>>, String>> {
        self.calls.lock().push("neighbor_memory_areas");
        let _ = caller;
        Box::pin(async { Ok(None) })
    }
    fn neighbor_memory_peer_uid<'a>(
        &'a self,
        authority: &'a NeighborAuthority,
    ) -> NeighborFuture<'a, Result<String, String>> {
        self.calls.lock().push("neighbor_memory_peer_uid");
        let _ = authority;
        Box::pin(async { Err("neighbors recording fixture has no configured response".into()) })
    }
    fn neighbor_memory_write_grant<'a>(
        &'a self,
        caller: &'a TabCaller,
    ) -> NeighborFuture<'a, Result<(NeighborAuthority, Permission), String>> {
        self.calls.lock().push("neighbor_memory_write_grant");
        let _ = caller;
        Box::pin(async { Err("neighbors recording fixture has no configured response".into()) })
    }
    fn neighbor_runtime_handling_event<'a>(
        &'a self,
        tab: &'a str,
        nonce: &'a str,
        thread: &'a str,
        turn: &'a str,
        message: Option<&'a str>,
        state: State,
    ) -> NeighborFuture<'a, codeterm_neighbors::Result<()>> {
        self.calls.lock().push("neighbor_runtime_handling_event");
        let _ = tab;
        let _ = nonce;
        let _ = thread;
        let _ = turn;
        let _ = message;
        let _ = state;
        Box::pin(async { Err(codeterm_neighbors::Error::Gateway) })
    }
    fn neighbor_scoped_tool_launch_args(
        &self,
        tab_id: &str,
        provider: &str,
        params: &SpawnAgentParams,
    ) -> std::result::Result<Vec<String>, String> {
        self.calls.lock().push("neighbor_scoped_tool_launch_args");
        let _ = tab_id;
        let _ = provider;
        let _ = params;
        Ok(Vec::new())
    }
    fn neighbor_spawn_friend_name(&self, target: &str) -> Option<String> {
        self.calls.lock().push("neighbor_spawn_friend_name");
        let _ = target;
        None
    }
    fn observe_neighbor_pty_handling(&self, tab: &str, status: TabStatus) {
        self.calls.lock().push("observe_neighbor_pty_handling");
        let _ = tab;
        let _ = status;
    }
    fn prepare_neighbor_restore_grant(
        &self,
        tab_id: &str,
        provider: &str,
        params: &SpawnAgentParams,
    ) -> std::result::Result<(), String> {
        self.calls.lock().push("prepare_neighbor_restore_grant");
        let _ = tab_id;
        let _ = provider;
        let _ = params;
        Ok(())
    }
    fn prepare_neighbor_root_close(&self, tab: &str) -> Result<Option<Mutation>, String> {
        self.calls.lock().push("prepare_neighbor_root_close");
        let _ = tab;
        Ok(None)
    }
    fn prepare_neighbor_tab_spawn(&self, spec: &mut SpawnTabSpec) -> Result<(), String> {
        self.calls.lock().push("prepare_neighbor_tab_spawn");
        let _ = spec;
        Ok(())
    }
    fn require_neighbor_restore_grant(
        &self,
        tab_id: &str,
        bypass: bool,
    ) -> std::result::Result<(), String> {
        self.calls.lock().push("require_neighbor_restore_grant");
        let _ = tab_id;
        let _ = bypass;
        Ok(())
    }
    fn session_revocations(
        &self,
    ) -> Arc<dyn codeterm_account::session_revocation::SessionRevocationObserver> {
        self.calls.lock().push("session_revocations");
        Arc::new(RecordingRevocations)
    }
    fn stamp_neighbor_message(&self, envelope: MessageEnvelope) -> MessageEnvelope {
        self.calls.lock().push("stamp_neighbor_message");
        envelope
    }
    fn unlock_account_mesh_after_plan_change(&self) {
        self.calls
            .lock()
            .push("unlock_account_mesh_after_plan_change");
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn neighbors_port_is_object_safe_and_thread_safe() {
        super::super::assert_port::<dyn super::NeighborsPort>();
    }
}

#[cfg(any(test, feature = "test-support"))]
struct RecordingRevocations;
#[cfg(any(test, feature = "test-support"))]
#[async_trait::async_trait]
impl codeterm_account::session_revocation::SessionRevocationObserver for RecordingRevocations {
    async fn revoked(&self, _: &codeterm_account::Session) {}
}
