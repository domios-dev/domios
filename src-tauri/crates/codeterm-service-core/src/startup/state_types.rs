use super::storage::StorageAuthority;
use crate::config::ConfigState;
use crate::multiplexer::commands::MuxState;
use crate::notification::NotifState;
use crate::session::SessionState;
use crate::{
    AgentTrackerState, KeepAwakeStateHandle, MeshStateHandle, OutputBufferState, PtyState,
    ReportStoreState, ResizeOwnerState, ScreenStateHandle, UpdateStateHandle,
};

/// All shared application states, grouped for easier passing between init functions.
pub struct AppStates {
    pub storage: StorageAuthority,
    pub boot_resync: bool,
    pub config: ConfigState,
    pub pty: PtyState,
    pub mux: MuxState,
    pub notif: NotifState,
    pub session: SessionState,
    pub output_buffer: OutputBufferState,
    pub screen_state: ScreenStateHandle,
    pub agent_tracker: AgentTrackerState,
    pub report_store: ReportStoreState,
    pub event_broadcaster: codeterm_events::EventBroadcasterState,
    pub update: UpdateStateHandle,
    pub mesh: MeshStateHandle,
    pub keepawake: KeepAwakeStateHandle,
    pub resize_owner: ResizeOwnerState,
    pub rest_api: super::RestApiControlState,
}
