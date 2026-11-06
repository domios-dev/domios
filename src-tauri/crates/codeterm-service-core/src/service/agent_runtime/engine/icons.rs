use super::*;
use std::io::Read;

fn registry_url(
    root: &std::path::Path,
    identity: Option<&str>,
    registry_id: Option<&str>,
) -> Option<String> {
    let file = std::fs::File::open(root.join("cache/registry-v1.json")).ok()?;
    let mut bytes = Vec::new();
    file.take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 2 * 1024 * 1024 {
        return None;
    }
    let index: Value = serde_json::from_slice(&bytes).ok()?;
    if index.get("version")?.as_str()? != "1.0.0" {
        return None;
    }
    let agents = index.get("agents")?.as_array()?;
    if agents.len() > 512 {
        return None;
    }
    let mut matching = agents.iter().filter(|agent| {
        if let Some(id) = registry_id {
            agent.get("id").and_then(Value::as_str) == Some(id)
        } else if let Some(identity) = identity {
            agent.get("id").and_then(Value::as_str) == Some(identity)
                || agent.get("name").and_then(Value::as_str) == Some(identity)
        } else {
            false
        }
    });
    let entry = matching.next()?;
    if matching.next().is_some() {
        return None;
    }
    Some(entry.get("icon")?.as_str()?.to_owned())
}

pub(super) fn start(svc: &ServiceContext, session: &Arc<Session>) {
    let svc = svc.clone();
    let session = session.clone();
    let context = session.context.lock().clone();
    let identity = session.state.lock().agent_name.clone();
    let epoch = session.state.lock().epoch;
    let profile = svc.mux.read().get_tab(&context.tab_id).and_then(|tab| {
        tab.agent_runtime_config
            .as_ref()?
            .get("launchProfile")?
            .as_str()
            .map(str::to_owned)
    });
    let declaration = session
        .adapter
        .lock()
        .as_ref()
        .and_then(|adapter| adapter.agent_runtime())
        .and_then(|runtime| runtime.launch_profiles?.get(profile.as_deref()?).cloned());
    if let Some(icon) = declaration
        .as_ref()
        .and_then(|profile| profile.icon_svg.clone())
        .filter(|icon| !icon.trim().is_empty())
    {
        publish_icon(&svc, &session, &context.tab_id, epoch, Some(icon));
        return;
    }
    crate::async_runtime::spawn(async move {
        let root = crate::paths::data_dir_path().join(&session.provider);
        let lookup_root = root.clone();
        let registry_id = declaration
            .as_ref()
            .and_then(|profile| profile.registry_id.clone());
        let url = declaration.and_then(|profile| profile.icon_url);
        let url = match url {
            Some(url) => Some(url),
            None => tokio::task::spawn_blocking(move || {
                registry_url(&lookup_root, identity.as_deref(), registry_id.as_deref())
            })
            .await
            .ok()
            .flatten(),
        };
        let Some(url) = url else {
            return;
        };
        let icon =
            codeterm_plugin::plugin_icon::registry::resolve(root.join("cache/icons"), &url).await;
        publish_icon(&svc, &session, &context.tab_id, epoch, icon);
    });
}

fn publish_icon(
    svc: &ServiceContext,
    session: &Session,
    tab: &str,
    epoch: u64,
    icon: Option<String>,
) {
    if validate_binding(svc, session).is_err() {
        return;
    }
    let applied = apply_icon(&mut session.state.lock(), epoch, icon);
    if applied {
        publish(svc, tab, session);
    }
}

fn apply_icon(state: &mut RuntimeState, epoch: u64, icon: Option<String>) -> bool {
    if state.epoch != epoch {
        return false;
    }
    let Some(icon) = icon.filter(|icon| !icon.trim().is_empty()) else {
        return false;
    };
    state.icon_svg = Some(icon);
    true
}

#[cfg(test)]
mod tests;
