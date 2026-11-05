use super::*;
use std::io::Read;

fn registry_url(root: &std::path::Path, identity: &str) -> Option<String> {
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
        agent.get("id").and_then(Value::as_str) == Some(identity)
            || agent.get("name").and_then(Value::as_str) == Some(identity)
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
    if declaration
        .as_ref()
        .is_some_and(|profile| profile.icon_svg.is_some())
    {
        return;
    }
    crate::async_runtime::spawn(async move {
        let root = crate::paths::data_dir_path().join(&session.provider);
        let lookup_root = root.clone();
        let url = declaration.and_then(|profile| profile.icon_url);
        let url = match url {
            Some(url) => Some(url),
            None => tokio::task::spawn_blocking(move || {
                registry_url(&lookup_root, identity.as_deref()?)
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
        if validate_binding(&svc, &session).is_err() {
            return;
        }
        {
            let mut state = session.state.lock();
            if state.epoch != epoch {
                return;
            }
            state.icon_svg = icon;
        }
        publish(&svc, &context.tab_id, &session);
    });
}

#[cfg(test)]
mod tests;
