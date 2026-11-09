use codeterm_types::{
    agent::chat_types::{ChatMsg, SystemSend},
    config::schema::AppConfig,
    pty::{
        AgentLiveState, AgentRole, AutoConfirmedPrompt, BootStage, PendingPrompt,
        ProvisionalLaunch, RemoteOwnership, TabStatus,
    },
    team::Team,
};
use ts_rs::{Config, TS};

pub fn export() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env().with_large_int("number");
    ChatMsg::export_all(&config)?;
    SystemSend::export_all(&config)?;
    if let Some(directory) = std::env::var_os("DOMIOS_PROTOCOL_EXPORT_DIR") {
        let protocol = config.with_out_dir(directory);
        codeterm_plugin::provider::ProviderDescriptor::export_all(&protocol)?;
        codeterm_plugin::provider::ModelGroupUsage::export_all(&protocol)?;
        codeterm_plugin::model_catalog::ModelCatalogueChanged::export_all(&protocol)?;
        codeterm_plugin::activity::PluginActivity::export_all(&protocol)?;
        codeterm_mesh::contract::types::MeshStatus::export_all(&protocol)?;
        codeterm_mesh::contract::types::MeshPeersResponse::export_all(&protocol)?;
        codeterm_mesh::network_transport::NetworkIdentity::export_all(&protocol)?;
        codeterm_worker_services::remote_workers::RemoteWorker::export_all(&protocol)?;
        codeterm_mem::workers::GraphWorkerInfo::export_all(&protocol)?;
        codeterm_agent_model::cron::store::CronRunInfo::export_all(&protocol)?;
        codeterm_agent_model::cron::store::CronJob::export_all(&protocol)?;
        codeterm_storage::CronCancelReason::export_all(&protocol)?;
        codeterm_mem::store::MemAuthor::export_all(&protocol)?;
        codeterm_mem::MemStats::export_all(&protocol)?;
        codeterm_mem::embedder::EmbedderStatus::export_all(&protocol)?;
        codeterm_mem::activity::MemActivityEvent::export_all(&protocol)?;
        codeterm_mem::graph_layout::snapshot::Snapshot::export_all(&protocol)?;
        codeterm_mem::graph_layout::snapshot::DeltaResponse::export_all(&protocol)?;
        AppConfig::export_all(&protocol)?;
        Team::export_all(&protocol)?;
        AgentRole::export_all(&protocol)?;
        PendingPrompt::export_all(&protocol)?;
        AutoConfirmedPrompt::export_all(&protocol)?;
        AgentLiveState::export_all(&protocol)?;
        BootStage::export_all(&protocol)?;
        ProvisionalLaunch::export_all(&protocol)?;
        RemoteOwnership::export_all(&protocol)?;
        TabStatus::export_all(&protocol)?;
        codeterm_types::pty::TabBootStageEvent::export_all(&protocol)?;
        codeterm_pty::multiplexer::types::Tab::export_all(&protocol)?;
        codeterm_pty::multiplexer::types::ClosedTab::export_all(&protocol)?;
        codeterm_pty::multiplexer::types::Workspace::export_all(&protocol)?;
        codeterm_pty::multiplexer::state::WorkspaceInfo::export_all(&protocol)?;
        codeterm_pty::multiplexer::state::ClosedStructureView::export_all(&protocol)?;
        codeterm_pty::multiplexer::state::MultiplexerSnapshot::export_all(&protocol)?;
        codeterm_types::pty::AtomicSpawn::<codeterm_pty::multiplexer::state::MultiplexerSnapshot>::export_all(&protocol)?;
    }
    Ok(())
}
