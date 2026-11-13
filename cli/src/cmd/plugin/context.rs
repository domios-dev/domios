use crate::args::plugin::{PluginAction, PluginChannelAction};
use codeterm_plugins::PluginCtx;
use serde_json::Value;
use std::path::PathBuf;

pub(super) fn resolve(action: &PluginAction) -> Result<PluginCtx, String> {
    resolve_with(crate::data_root(), action, || {
        let (mut client, _) = crate::client::SocketClient::connect_local()?;
        client.call("system.identify", None)
    })
}

fn resolve_with(
    data_root: PathBuf,
    action: &PluginAction,
    identify: impl FnOnce() -> Result<Value, String>,
) -> Result<PluginCtx, String> {
    let required = matches!(
        action,
        PluginAction::Install { .. }
            | PluginAction::Update { .. }
            | PluginAction::UpdateSystem { .. }
    );
    let version = if required
        || matches!(
            action,
            PluginAction::List
                | PluginAction::Channel {
                    command: PluginChannelAction::Diff { .. }
                }
        ) {
        let reported = identify().and_then(|identity| {
            identity["version"]
                .as_str()
                .filter(|version| !version.trim().is_empty())
                .map(str::to_owned)
                .ok_or_else(|| "system.identify did not report the running Domios version".into())
        });
        match reported {
            Ok(version) => Some(version),
            Err(_) if !required => None,
            Err(error) => return Err(error),
        }
    } else {
        None
    };
    Ok(PluginCtx { data_root, version })
}

#[cfg(test)]
mod tests;
