use crate::{
    client::{RpcClient, SocketClient},
    Commands, MemAction, PluginAction, SystemAction,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Operation {
    Maintenance,
    HostOwner,
    LocalInput,
}

impl Operation {
    fn label(self) -> &'static str {
        match self {
            Self::Maintenance => "maintenance",
            Self::HostOwner => "host-owner",
            Self::LocalInput => "local-input",
        }
    }
}

pub(crate) fn require(operation: Operation) -> Result<(), String> {
    let (mut client, _) = SocketClient::connect_local()?;
    require_rpc(&mut client, operation)
}

fn require_rpc(client: &mut dyn RpcClient, operation: Operation) -> Result<(), String> {
    let result = client.call(
        "cli.local.authorize",
        Some(serde_json::json!({"operation":operation.label()})),
    )?;
    if result.get("authorized") == Some(&serde_json::Value::Bool(true)) {
        Ok(())
    } else {
        Err("Local CLI operation was not authorized".into())
    }
}

pub(crate) fn host_scoped() -> bool {
    std::env::var_os("CODETERM_CLI_SCOPE").is_some()
}

pub(crate) fn requires_admission(scoped: bool) -> bool {
    admission_required(
        scoped,
        crate::nonempty_env("CODETERM_API_TOKEN").is_some(),
        crate::nonempty_env("CODETERM_TAB_ID").is_some(),
    )
}

fn admission_required(scoped: bool, inherited_token: bool, inherited_tab: bool) -> bool {
    scoped || inherited_token || inherited_tab
}

pub(crate) fn read_local_text(path: impl AsRef<std::path::Path>) -> std::io::Result<String> {
    read_local_text_with(path.as_ref(), || {
        if host_scoped() {
            require(Operation::LocalInput)
        } else {
            Ok(())
        }
    })
}

fn read_local_text_with(
    path: &std::path::Path,
    authorize: impl FnOnce() -> Result<(), String>,
) -> std::io::Result<String> {
    authorize()
        .map_err(|reason| std::io::Error::new(std::io::ErrorKind::PermissionDenied, reason))?;
    std::fs::read_to_string(path)
}

pub(crate) fn read_local_bytes(path: impl AsRef<std::path::Path>) -> std::io::Result<Vec<u8>> {
    if host_scoped() {
        require(Operation::LocalInput)
            .map_err(|reason| std::io::Error::new(std::io::ErrorKind::PermissionDenied, reason))?;
    }
    std::fs::read(path)
}

pub(crate) fn operation_for_cli(cli: &crate::Cli, scoped: bool) -> Option<Operation> {
    if scoped && cli.mesh.is_some() {
        return Some(Operation::Maintenance);
    }
    operation_for(&cli.command, scoped)
}

pub(crate) fn operation_for(command: &Commands, _scoped: bool) -> Option<Operation> {
    match command {
        Commands::System {
            action: SystemAction::ApiToken,
        } => Some(Operation::HostOwner),
        Commands::System { .. } => Some(Operation::HostOwner),
        Commands::Plugin {
            action: PluginAction::Pass(_) | PluginAction::ReceiverTarget { .. },
        } => None,
        Commands::Plugin { .. }
        | Commands::Trace { .. }
        | Commands::EssenceMap { .. }
        | Commands::Plan { .. }
        | Commands::Update { .. }
        | Commands::Agent {
            action: crate::AgentAction::Report(_),
        }
        | Commands::CliManifest { write: true, .. } => Some(Operation::Maintenance),
        Commands::Mem {
            action:
                MemAction::Backup { .. }
                | MemAction::Backend { .. }
                | MemAction::RunWorker { .. }
                | MemAction::Dream { .. }
                | MemAction::Embedder {
                    action: crate::MemEmbedderAction::Key { .. },
                }
                | MemAction::Skill { .. },
        } => Some(Operation::Maintenance),
        Commands::Mem {
            action: MemAction::Graph { action },
        } => match action {
            crate::MemGraphAction::Route { .. }
            | crate::MemGraphAction::PurgeInvalidated { .. }
            | crate::MemGraphAction::ReviveBroken { .. }
            | crate::MemGraphAction::IntroduceTopic { .. }
            | crate::MemGraphAction::Hubs { .. }
            | crate::MemGraphAction::Islands { .. }
            | crate::MemGraphAction::Orphans { .. }
            | crate::MemGraphAction::Get { .. } => Some(Operation::Maintenance),
            _ => None,
        },
        Commands::Blueprint { action } => match action {
            crate::args::blueprint::BlueprintAction::Tags
            | crate::args::blueprint::BlueprintAction::Schema
            | crate::args::blueprint::BlueprintAction::Recipe { .. }
            | crate::args::blueprint::BlueprintAction::New {
                like: None,
                intent: None,
                ..
            } => None,
            _ => Some(Operation::LocalInput),
        },
        Commands::Account { .. }
        | Commands::Analytics { .. }
        | Commands::Config { .. }
        | Commands::Supporter { .. }
        | Commands::Docs { .. }
        | Commands::CliManifest { write: false, .. } => None,
        _ => match crate::transport::transport_for(command).local {
            crate::transport::Local::PreMesh
            | crate::transport::Local::PreSocket
            | crate::transport::Local::PreSocketOrSocket => Some(Operation::Maintenance),
            crate::transport::Local::Socket => None,
        },
    }
}

#[cfg(test)]
mod tests;
