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
