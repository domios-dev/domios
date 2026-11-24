use crate::prelude::*;
use codeterm_account::account_mesh::AccountMesh;
use codeterm_neighbors::permissions::Permission;
use codeterm_service_core::ports::neighbors::{NeighborFuture, NeighborsPort};
use codeterm_service_core::service::ExtensionsBuilder;
use codeterm_service_core::service::ServiceContext;
use codeterm_service_core::service::{
    messenger::MessageEnvelope,
    neighbor_state::{Mutation, NeighborAuthority},
    spawn::SpawnTabSpec,
    tab_credentials::TabCaller,
    ServiceState, SpawnAgentParams,
};
use codeterm_types::pty::TabStatus;
use codeterm_wire::api_contracts::State;
use std::sync::{Arc, Weak};
struct NeighborPortImpl(Weak<ServiceContext>);
impl NeighborPortImpl {
    fn svc(&self) -> ServiceState {
        self.0
            .upgrade()
            .expect("neighbors port outlived its ServiceContext")
    }
}
pub fn register_neighbors() {
    codeterm_service_core::ports::neighbors::register_neighbors(
        |svc| Arc::new(NeighborPortImpl(Arc::downgrade(&svc))),
        register_extensions,
    );
}
fn register_extensions(builder: &mut ExtensionsBuilder) {
    builder.insert_late::<crate::service::neighbors::NeighborRuntimeCell>();
    builder.insert_late::<crate::service::neighbors::updates::UpdateCell>();
    builder.insert_late::<crate::service::neighbors::history::HistoryCell>();
    builder.insert_late::<crate::service::account::AccountCredentialCell>();
    builder.insert_late::<crate::service::account::callable_admission::Changes>();
    builder.insert_late::<crate::service::account::startup::IdentityWriter>();
    builder.insert_late::<crate::service::account::logout::LogoutCell>();
    builder.insert_late::<crate::service::account_mesh::AccountMeshCell>();
    #[cfg(any(test, feature = "test-support"))]
    builder.insert_late::<crate::service::neighbors::runtime::logout::LogoutKeyPorts>();
}
impl NeighborsPort for NeighborPortImpl {
    fn neighbor_read_only_reply_scope<'a>(
        &'a self,
        caller: &'a TabCaller,
        friendship: &'a str,
    ) -> NeighborFuture<'a, Result<Option<String>, String>> {
        let svc = self.svc();
        Box::pin(async move {
            Ok(svc
                .neighbor_capability_permission_for(caller, false)
                .await?
                .filter(|(id, permission)| {
                    id == friendship
                        && permission.allowed
                        && permission.machine_access
                            == codeterm_neighbors::permissions::MachineAccess::ReadOnly
                })
                .map(|(_, permission)| permission.scope))
        })
    }

    fn account<'a>(&'a self) -> NeighborFuture<'a, Result<Arc<codeterm_account::Account>, String>> {
        let svc = self.svc();
        Box::pin(async move { svc.account().await })
    }
    fn account_mesh<'a>(&'a self) -> NeighborFuture<'a, Result<Arc<AccountMesh>, String>> {
        let svc = self.svc();
        Box::pin(async move { svc.account_mesh().await.map(|mesh| mesh.mesh.clone()) })
    }
    fn authorize_neighbor_rpc_credential<'a>(
        &'a self,
        credential: &'a str,
        method: &'a str,
        params: Option<&'a serde_json::Value>,
    ) -> NeighborFuture<'a, Result<(), String>> {
        let svc = self.svc();
        Box::pin(async move {
            svc.authorize_neighbor_rpc_credential(credential, method, params)
                .await
        })
    }
    fn authorize_neighbor_secret_rpc(
        &self,
        caller: &TabCaller,
        method: &str,
        params: Option<&serde_json::Value>,
    ) -> Result<(), String> {
        self.svc()
            .authorize_neighbor_secret_rpc(caller, method, params)
    }
    fn carry_neighbor_restore_grant(
        &self,
        tab_id: &str,
        old_nonce: &str,
        new_nonce: &str,
    ) -> std::result::Result<(), String> {
        self.svc()
            .carry_neighbor_restore_grant(tab_id, old_nonce, new_nonce)
    }
    fn inherit_neighbor_agent_spawn<'a>(
        &'a self,
        params: &'a mut SpawnAgentParams,
    ) -> NeighborFuture<'a, Result<bool, String>> {
        let svc = self.svc();
        Box::pin(async move { svc.inherit_neighbor_agent_spawn(params).await })
    }
    fn inherit_neighbor_agent_spawn_sync(
        &self,
        params: &mut SpawnAgentParams,
    ) -> Result<bool, String> {
        self.svc().inherit_neighbor_agent_spawn_sync(params)
    }
    fn neighbor_entitlements_changed(&self, changed: bool) {
        self.svc().neighbor_entitlements_changed(changed)
    }
    fn neighbor_friend_display_name(&self, friendship_id: &str) -> Option<String> {
        self.svc().neighbor_friend_display_name(friendship_id)
    }
    fn neighbor_launch_caller(
        &self,
        callers: &[Option<&str>],
    ) -> Result<Option<TabCaller>, String> {
        self.svc().neighbor_launch_caller(callers)
    }
    fn neighbor_memory_areas<'a>(
        &'a self,
        caller: &'a TabCaller,
    ) -> NeighborFuture<'a, Result<Option<Vec<String>>, String>> {
        let svc = self.svc();
        Box::pin(async move { svc.neighbor_memory_areas(caller).await })
    }
    fn neighbor_memory_peer_uid<'a>(
        &'a self,
        authority: &'a NeighborAuthority,
    ) -> NeighborFuture<'a, Result<String, String>> {
        let svc = self.svc();
        Box::pin(async move { svc.neighbor_memory_peer_uid(authority).await })
    }
    fn neighbor_memory_write_grant<'a>(
        &'a self,
        caller: &'a TabCaller,
    ) -> NeighborFuture<'a, Result<(NeighborAuthority, Permission), String>> {
        let svc = self.svc();
        Box::pin(async move { svc.neighbor_memory_write_grant(caller).await })
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
        let svc = self.svc();
        Box::pin(async move {
            svc.neighbor_runtime_handling_event(tab, nonce, thread, turn, message, state)
                .await
        })
    }
    fn neighbor_scoped_tool_launch_args(
        &self,
        tab_id: &str,
        provider: &str,
        params: &SpawnAgentParams,
    ) -> std::result::Result<Vec<String>, String> {
        self.svc()
            .neighbor_scoped_tool_launch_args(tab_id, provider, params)
    }
    fn neighbor_spawn_friend_name(&self, target: &str) -> Option<String> {
        self.svc().neighbor_spawn_friend_name(target)
    }
    fn observe_neighbor_pty_handling(&self, tab: &str, status: TabStatus) {
        self.svc().observe_neighbor_pty_handling(tab, status)
    }
    fn prepare_neighbor_restore_grant(
        &self,
        tab_id: &str,
        provider: &str,
        params: &SpawnAgentParams,
    ) -> std::result::Result<(), String> {
        self.svc()
            .prepare_neighbor_restore_grant(tab_id, provider, params)
    }
    fn prepare_neighbor_root_close(&self, tab: &str) -> Result<Option<Mutation>, String> {
        self.svc().prepare_neighbor_root_close(tab)
    }
    fn prepare_neighbor_tab_spawn(&self, spec: &mut SpawnTabSpec) -> Result<(), String> {
        self.svc().prepare_neighbor_tab_spawn(spec)
    }
    fn require_neighbor_restore_grant(
        &self,
        tab_id: &str,
        bypass: bool,
    ) -> std::result::Result<(), String> {
        self.svc().require_neighbor_restore_grant(tab_id, bypass)
    }
    fn session_revocations(
        &self,
    ) -> Arc<dyn codeterm_account::session_revocation::SessionRevocationObserver> {
        self.svc().session_revocations()
    }
    fn stamp_neighbor_message(&self, envelope: MessageEnvelope) -> MessageEnvelope {
        self.svc().stamp_neighbor_message(envelope)
    }
    fn unlock_account_mesh_after_plan_change(&self) {
        self.svc().unlock_account_mesh_after_plan_change()
    }
}
