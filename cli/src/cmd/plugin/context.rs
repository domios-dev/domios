use crate::args::plugin::{PluginAction, PluginChannelAction};
use codeterm_plugins::PluginCtx;
use serde_json::Value;
use std::path::PathBuf;

pub(super) fn resolve(
    action: &PluginAction,
) -> Result<(PluginCtx, Option<crate::client::SocketClient>), String> {
    if matches!(
        action,
        PluginAction::Install { .. }
            | PluginAction::Update { .. }
            | PluginAction::UpdateSystem { .. }
    ) {
        return mutation_context(
            crate::data_root(),
            action,
            crate::client::SocketClient::connect_optional_local(),
            |client| client.call("system.identify", None),
        );
    }
    resolve_with(crate::data_root(), action, || {
        let (mut client, _) = crate::client::SocketClient::connect_local()?;
        client.call("system.identify", None)
    })
    .map(|ctx| (ctx, None))
}

fn mutation_context<T>(
    data_root: PathBuf,
    action: &PluginAction,
    connection: Result<Option<T>, String>,
    identify: impl FnOnce(&mut T) -> Result<Value, String>,
) -> Result<(PluginCtx, Option<T>), String> {
    match connection? {
        Some(mut client) => {
            let ctx = resolve_with(data_root, action, || identify(&mut client))?;
            Ok((ctx, Some(client)))
        }
        None => Ok((
            PluginCtx {
                data_root,
                version: None,
            },
            None,
        )),
    }
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
