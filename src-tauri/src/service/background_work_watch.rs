use super::ServiceContext;
use codeterm_types::agent::chat_types::RichMessage;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::Path,
    sync::{mpsc, OnceLock},
};
use tauri::Manager;
struct Watch {
    identity: (String, String),
    _watcher: Option<RecommendedWatcher>,
}
fn watches() -> &'static Mutex<HashMap<String, Watch>> {
    static WATCHES: OnceLock<Mutex<HashMap<String, Watch>>> = OnceLock::new();
    WATCHES.get_or_init(Mutex::default)
}
pub(crate) fn cancel(tab: &str) {
    let old = { watches().lock().remove(tab) };
    drop(old);
}
pub(crate) fn ensure(svc: &ServiceContext, tab: &str) {
    let target = {
        let mux = svc.mux.read();
        mux.get_tab(tab).and_then(|tab| {
            Some((
                tab.agent_type.clone()?,
                tab.cwd.clone()?,
                tab.agent_session_id.clone()?,
                tab.session_binding_nonce.clone()?,
            ))
        })
    };
    let Some((provider, cwd, session, nonce)) = target else {
        cancel(tab);
        return;
    };
    let enabled = svc
        .app_handle
        .try_state::<crate::config::ConfigState>()
        .is_some_and(|config| {
            config.lock().agents.get(&provider).is_some_and(|config| {
                [
                    "background_work_guard",
                    "structured_signals",
                    "capability_probe_enabled",
                ]
                .iter()
                .any(|key| config.extra.get(*key).and_then(toml::Value::as_bool) == Some(true))
            })
        });
    if !enabled
        || !svc
            .agent_registry
            .with_provider(&provider, |provider| {
                provider.tracks_background_work() || provider.auth_status_probe().is_some()
            })
            .unwrap_or(false)
    {
        cancel(tab);
        return;
    }
    let queue = svc.pending_delivery_payloads.user_sends.tab(tab);
    let old = {
        let mut watches = watches().lock();
        if queue.revoked.load(std::sync::atomic::Ordering::Acquire)
            || watches
                .get(tab)
                .is_some_and(|watch| watch.identity == (session.clone(), nonce.clone()))
        {
            return;
        }
        watches.insert(
            tab.into(),
            Watch {
                identity: (session.clone(), nonce.clone()),
                _watcher: None,
            },
        )
    };
    drop(old);
    mark_unavailable(svc, tab, &session, &nonce);
    let svc = svc.clone();
    let tab = tab.to_string();
    if let Err(error) = std::thread::Builder::new()
        .name("background-transcript-watch".into())
        .spawn(move || {
            let path = svc
                .agent_registry
                .with_provider(&provider, |provider| {
                    provider.session_jsonl_path(&cwd, &session)
                })
                .flatten();
            let Some(path) = path else {
                return;
            };
            let Some(directory) = path
                .ancestors()
                .skip(1)
                .take(3)
                .find(|path| path.is_dir())
                .map(Path::to_path_buf)
            else {
                return;
            };
            let (tx, rx) = mpsc::sync_channel(1);
            let watched = path.clone();
            let events = tx.clone();
            let watcher =
                notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                    if event.is_ok_and(|event| {
                        if !matches!(
                            event.kind,
                            notify::EventKind::Create(_)
                                | notify::EventKind::Modify(_)
                                | notify::EventKind::Remove(_)
                        ) {
                            return false;
                        }
                        event
                            .paths
                            .iter()
                            .any(|path| path == &watched || watched.starts_with(path))
                    }) {
                        let _ = events.try_send(());
                    }
                });
            let Ok(mut watcher) = watcher else {
                return;
            };
            if watcher.watch(&directory, RecursiveMode::Recursive).is_err() {
                return;
            }
            {
                let mut watches = watches().lock();
                let Some(entry) = watches
                    .get_mut(&tab)
                    .filter(|watch| watch.identity == (session.clone(), nonce.clone()))
                else {
                    return;
                };
                entry._watcher = Some(watcher);
            }
            let _ = tx.try_send(());
            drop(tx);
            let mut offset = 0;
            let mut incomplete_history = false;
            while rx.recv().is_ok() {
                let current = svc
                    .mux
                    .read()
                    .get_tab(&tab)
                    .filter(|tab| {
                        tab.agent_session_id.as_deref() == Some(&session)
                            && tab.session_binding_nonce.as_deref() == Some(&nonce)
                    })
                    .map(|tab| tab.fork_source_session_id.is_some());
                let Some(fork) = current else {
                    break;
                };
                match read(&svc, &provider, &path, offset) {
                    Ok((mut records, end, incomplete, partial)) => {
                        records.retain(|row| {
                            row.provider_session_id
                                .as_deref()
                                .is_none_or(|id| id == session)
                                && (!fork || row.provider_session_id.as_deref() == Some(&session))
                        });
                        super::background_work::apply(&svc, &tab, &session, &nonce, &records);
                        super::provider_signals::apply(&svc, &tab, &session, &nonce, &records);
                        if let Some(context) = records
                            .iter()
                            .rev()
                            .find_map(|record| record.context_usage.as_ref())
                        {
                            crate::api::hook_routes::context_feed::apply_snapshot(
                                &svc,
                                &tab,
                                &session,
                                Some(&nonce),
                                Some(context),
                            );
                        }
                        offset = end;
                        incomplete_history |= incomplete;
                        if partial {
                            mark_unavailable(&svc, &tab, &session, &nonce);
                        } else {
                            set_initialized(&svc, &tab, &session, &nonce, !incomplete_history);
                        }
                    }
                    Err(_) => {
                        mark_unavailable(&svc, &tab, &session, &nonce);
                    }
                }
            }
        })
    {
        log::warn!("[background-work] watcher could not start: {error}");
    }
}
fn set_initialized(svc: &ServiceContext, id: &str, session: &str, nonce: &str, complete: bool) {
    let changed = {
        let mut mux = svc.mux.write();
        let Some(tab) = mux.get_tab_mut(id).filter(|tab| {
            tab.agent_session_id.as_deref() == Some(session)
                && tab.session_binding_nonce.as_deref() == Some(nonce)
        }) else {
            return;
        };
        let before = tab.provider_signals.background.clone();
        tab.provider_signals.background.history_initialized = true;
        tab.provider_signals.background.history_incomplete = !complete;
        before != tab.provider_signals.background
    };
    if changed {
        svc.emit_state_changed();
        super::provider_update::background_work_changed(svc, id);
    }
}
fn read(
    svc: &ServiceContext,
    provider: &str,
    path: &Path,
    offset: u64,
) -> std::io::Result<(Vec<RichMessage>, u64, bool, bool)> {
    read_file(path, offset, |value| {
        svc.agent_registry
            .with_provider(provider, |provider| RichMessage {
                provider_session_id: value
                    .get("sessionId")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
                provider_observations: provider.parse_session_observations(value),
                background_work: provider.parse_background_work(value),
                context_usage: provider.parse_context_usage(value),
                ..Default::default()
            })
    })
}
fn read_file(
    path: &Path,
    offset: u64,
    decode: impl Fn(&serde_json::Value) -> Option<RichMessage>,
) -> std::io::Result<(Vec<RichMessage>, u64, bool, bool)> {
    const BOUND: u64 = 8 * 1024 * 1024;
    let mut file = std::fs::File::open(path)?;
    let total = file.metadata()?.len();
    let start = if offset > total { 0 } else { offset };
    let mut incomplete = offset > total || total.saturating_sub(start) > BOUND;
    let start = if incomplete {
        codeterm_plugin::jsonl_window::tail_window_start(path, total, BOUND)
    } else {
        start
    };
    file.seek(SeekFrom::Start(start))?;
    let mut reader = BufReader::new(file.take(BOUND.min(total.saturating_sub(start))));
    let mut records = Vec::new();
    let mut end = start;
    loop {
        let mut line = String::new();
        let count = reader.read_line(&mut line)?;
        if count == 0 || !line.ends_with('\n') {
            break;
        }
        end += count as u64;
        let Ok(value) = serde_json::from_str(&line) else {
            incomplete = true;
            continue;
        };
        if let Some(record) = decode(&value) {
            if record.provider_observations.is_some()
                || !record.background_work.is_empty()
                || record.context_usage.is_some()
            {
                records.push(record);
            }
        }
    }
    Ok((records, end, incomplete, end < total))
}

fn mark_unavailable(svc: &ServiceContext, id: &str, session: &str, nonce: &str) {
    let changed = {
        let mut mux = svc.mux.write();
        let Some(tab) = mux.get_tab_mut(id).filter(|tab| {
            tab.agent_session_id.as_deref() == Some(session)
                && tab.session_binding_nonce.as_deref() == Some(nonce)
        }) else {
            return;
        };
        let changed = tab.provider_signals.background.history_initialized;
        tab.provider_signals.background.history_initialized = false;
        changed
    };
    if changed {
        svc.emit_state_changed();
        super::provider_update::background_work_changed(svc, id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn decode(value: &serde_json::Value) -> Option<RichMessage> {
        Some(RichMessage {
            provider_observations: Some(
                codeterm_types::agent::provider_signals::SessionObservations {
                    permission_mode: Some(
                        codeterm_types::agent::provider_signals::PermissionMode::Plan,
                    ),
                    ..Default::default()
                },
            ),
            provider_session_id: value["sessionId"].as_str().map(str::to_string),
            ..Default::default()
        })
    }
    #[test]
    fn restart_guard_event_reader_holds_partial_records_and_advances_once() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        write!(
            file,
            "{{\"sessionId\":\"own\"}}\n{{\"sessionId\":\"next\"}}"
        )
        .unwrap();
        file.flush().unwrap();
        let (rows, offset, incomplete, partial) = read_file(file.path(), 0, decode).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(!incomplete);
        assert!(partial);
        assert_eq!(rows[0].provider_session_id.as_deref(), Some("own"));
        let (rows, held, _, _) = read_file(file.path(), offset, decode).unwrap();
        assert!(rows.is_empty());
        assert_eq!(held, offset);
        writeln!(file).unwrap();
        file.flush().unwrap();
        let (rows, end, _, partial) = read_file(file.path(), offset, decode).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(end > offset);
        assert!(!partial);
        assert!(read_file(file.path(), end, decode).unwrap().0.is_empty());
    }
    #[test]
    fn restart_guard_event_reader_truncation_fails_closed() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{{\"sessionId\":\"own\"}}").unwrap();
        file.flush().unwrap();
        assert!(read_file(file.path(), 1000, decode).unwrap().2);
    }
}
