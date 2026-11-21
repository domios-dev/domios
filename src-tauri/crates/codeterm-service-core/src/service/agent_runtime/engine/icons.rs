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
    let has_local_icon = declaration
        .as_ref()
        .is_some_and(|profile| profile.icon.is_some());
    if let Some(icon) = declaration
        .as_ref()
        .and_then(|profile| profile.icon_svg.clone())
        .filter(|icon| !icon.trim().is_empty())
    {
        publish_icon(&svc, &session, &context.tab_id, epoch, Some(icon));
        if has_local_icon
            || declaration
                .as_ref()
                .and_then(|profile| profile.icon_url.as_ref())
                .is_none()
        {
            return;
        }
    }
    if has_local_icon {
        return;
    }
    crate::async_runtime::spawn(async move {
        let root = crate::paths::data_dir_path().join(&session.provider);
        let cache_dir =
            codeterm_plugin::agent_runtime::owner_profiles::icon_cache_dir(&session.provider)
                .unwrap_or_else(|_| root.join("cache/icons"));
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
        let icon = codeterm_plugin::plugin_icon::registry::resolve(cache_dir, &url).await;
        if let Some(icon) = icon.as_ref() {
            let provider = session.provider.clone();
            let url = url.clone();
            let icon = icon.clone();
            let projection = tokio::task::spawn_blocking(move || {
                codeterm_plugin::agent_runtime::owner_profiles::refresh_icon(&provider, &url, &icon)
            })
            .await;
            match projection {
                Ok(Ok(true)) => {
                    svc.provider_registry.recheck_profiles(&session.provider);
                    svc.events
                        .publish(codeterm_events::SystemEvent::ContributionsChanged {
                            kind: "agent_provider".into(),
                        });
                }
                Ok(Ok(false)) => {}
                Ok(Err(error)) => log::debug!("[runtime.icon] profile projection failed: {error}"),
                Err(error) => log::debug!("[runtime.icon] profile projection task failed: {error}"),
            }
        }
        publish_icon(&svc, &session, &context.tab_id, epoch, icon);
    });
}

fn resolve_profile_icon_url(
    registry_root: &std::path::Path,
    profile: &RuntimeLaunchProfile,
) -> Option<String> {
    if profile.icon.is_some() {
        return None;
    }
    profile
        .registry_id
        .as_deref()
        .and_then(|id| registry_url(registry_root, None, Some(id)))
        .filter(|url| codeterm_plugin::plugin_icon::registry::valid_url(url))
        .or_else(|| {
            profile
                .icon_url
                .clone()
                .filter(|url| codeterm_plugin::plugin_icon::registry::valid_url(url))
        })
}

pub async fn refresh_provider_profile_icons(svc: &ServiceContext, provider: &str) -> usize {
    let has_profiles = svc
        .agent_registry
        .with_provider(provider, |p| {
            p.agent_runtime()
                .is_some_and(|runtime| runtime.launch_profiles.is_some())
        })
        .unwrap_or(false);
    if !has_profiles {
        return 0;
    }
    let provider_name = provider.to_owned();
    let root = crate::paths::data_dir_path().join(provider);
    let lookup_root = root.clone();
    let Ok(Ok(targets)) = tokio::task::spawn_blocking(move || {
        codeterm_plugin::agent_runtime::owner_profiles::load(&provider_name).map(|profiles| {
            profiles
                .into_iter()
                .filter_map(|(id, profile)| {
                    let url = resolve_profile_icon_url(&lookup_root, &profile)?;
                    Some((id, url))
                })
                .collect::<Vec<_>>()
        })
    })
    .await
    else {
        return 0;
    };
    let cache_dir = codeterm_plugin::agent_runtime::owner_profiles::icon_cache_dir(provider)
        .unwrap_or_else(|_| root.join("cache/icons"));
    let mut refreshed = 0;
    for (profile_id, url) in targets {
        let Some(icon) =
            codeterm_plugin::plugin_icon::registry::resolve(cache_dir.clone(), &url).await
        else {
            continue;
        };
        let provider_name = provider.to_owned();
        let sync_url = url.clone();
        let sync_icon = icon.clone();
        let updated = tokio::task::spawn_blocking(move || {
            let _ = codeterm_plugin::agent_runtime::owner_profiles::sync_registry_icon_url(
                &provider_name,
                &profile_id,
                &sync_url,
            );
            codeterm_plugin::agent_runtime::owner_profiles::refresh_icon(
                &provider_name,
                &sync_url,
                &sync_icon,
            )
        })
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or(false);
        if updated {
            refreshed += 1;
        }
    }
    if refreshed > 0 {
        svc.provider_registry.recheck_profiles(provider);
        svc.events
            .publish(codeterm_events::SystemEvent::ContributionsChanged {
                kind: "agent_provider".into(),
            });
    }
    refreshed
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
