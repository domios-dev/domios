use super::*;

fn changed_target(
    baseline: &[u8],
    current: &[u8],
    requested: &str,
    observed: Option<&str>,
) -> bool {
    baseline != current && observed == Some(requested)
}

fn own_file_event(path: &Path, expected: &Path) -> bool {
    path.canonicalize().is_ok_and(|path| path == expected)
        || (path.file_name() == expected.file_name()
            && path
                .parent()
                .and_then(|parent| parent.canonicalize().ok())
                .as_deref()
                == expected.parent())
}

fn identity_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    if file.metadata().map_err(|error| error.to_string())?.len() > MAX_CONFIRMATION_BYTES {
        return Err("the session identity exceeds the confirmation bound".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_CONFIRMATION_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    Ok(bytes)
}

impl ServiceContext {
    pub(super) fn run_session_identity_command(
        &self,
        target: &CommandTarget,
        effort: Option<&str>,
        template: &ModelSwitchCommand,
    ) -> Result<(), ModelSwitchError> {
        let fail = ModelSwitchError::LiveCommandFailed;
        if template.prompt.is_some() || template.picker.is_some() {
            return Err(fail(
                "session identity commands do not support interactive pickers".into(),
            ));
        }
        let cwd = target
            .cwd
            .as_deref()
            .ok_or_else(|| fail("the session cwd is unavailable".into()))?;
        let command = render_command(&template.command, &target.model, effort).map_err(fail)?;
        let path = self
            .agent_registry
            .with_provider(&target.provider, |provider| {
                provider.session_model_identity_path(cwd, &target.session_id)
            })
            .flatten()
            .ok_or_else(|| fail("the bound session has no permitted identity file".into()))?;
        let observed = || {
            self.agent_registry
                .with_provider(&target.provider, |provider| {
                    if effort.is_some() {
                        provider.detect_session_reasoning_effort(cwd, &target.session_id)
                    } else {
                        provider.detect_session_model(cwd, &target.session_id)
                    }
                })
                .flatten()
        };
        let requested = effort.unwrap_or(&target.model);
        let model_current = || {
            effort.is_none()
                || self
                    .agent_registry
                    .with_provider(&target.provider, |provider| {
                        provider.detect_session_model(cwd, &target.session_id)
                    })
                    .flatten()
                    .as_deref()
                    == Some(target.model.as_str())
        };
        let deadline = Instant::now() + Duration::from_secs(MODEL_OBSERVE_TIMEOUT_SECS);
        let idle = || {
            target.current(self)
                && self
                    .mux
                    .read()
                    .get_tab(&target.tab_id)
                    .is_some_and(|tab| tab.pending_prompt.is_none())
                && self.pty.lock().get_session(&target.pty_id).is_some()
                && crate::service::status_reconciliation::snapshot_with_reconciled_status_until(
                    &self.mux,
                    &self.agent_tracker,
                    &self.agent_registry,
                    deadline,
                )
                .is_some_and(|snapshot| {
                    snapshot.tab_statuses.get(&target.tab_id)
                        == Some(&codeterm_types::pty::TabStatus::Idle)
                })
        };
        if !idle() {
            return Err(fail("the agent is busy or waiting on a prompt".into()));
        }
        if !model_current() {
            return Err(fail(
                "the session model does not match the requested effort's model".into(),
            ));
        }
        if observed().as_deref() == Some(requested) {
            return Ok(());
        }
        let signal = self
            .screen_state
            .lock()
            .arm_screen_change_signal(&target.pty_id);
        crate::service::boot_wait::observe_boot_changes(&signal);
        let result = (|| -> Result<(), String> {
            let wake = signal.clone();
            let watched_path = path.clone();
            let mut watcher =
                notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                    if event.is_ok_and(|event| {
                        event
                            .paths
                            .iter()
                            .any(|path| own_file_event(path, &watched_path))
                    }) {
                        wake.mark_changed();
                    }
                })
                .map_err(|error| error.to_string())?;
            let parent = path
                .parent()
                .ok_or_else(|| "the identity file has no parent".to_owned())?;
            notify::Watcher::watch(&mut watcher, parent, notify::RecursiveMode::NonRecursive)
                .map_err(|error| error.to_string())?;
            let baseline = identity_bytes(&path)?;
            let injected = std::cell::Cell::new(false);
            let result = execute_command(
                idle,
                || {
                    if identity_bytes(&path)? != baseline {
                        return Err(
                            "the session identity changed before command injection; select again"
                                .into(),
                        );
                    }
                    let _claim = crate::service::pty_io::submit_transaction::claim_submit_cycle(
                        &target.pty_id,
                    );
                    let (session, permit, input) = {
                        let mux = self.mux.read();
                        if !mux
                            .get_tab(&target.tab_id)
                            .is_some_and(|tab| target.matches(tab))
                        {
                            return Err("the session changed before command injection".into());
                        }
                        let screen = self.screen_state.lock();
                        let input = crate::service::pty_io::submit_transaction::live_command_input(
                            &command,
                            screen.bracketed_paste_mode(&target.pty_id),
                        );
                        let pty = self.pty.lock();
                        let session = pty
                            .get_session(&target.pty_id)
                            .ok_or_else(|| "the command PTY closed".to_owned())?;
                        let permit = pty.issue_guarded_input_permit(&target.pty_id)?;
                        (session, permit, input)
                    };
                    transport::enqueue_command(
                        &session.write_queue,
                        permit,
                        input,
                        deadline.saturating_duration_since(Instant::now()),
                        &injected,
                    )
                },
                || loop {
                    let generation = signal.generation();
                    if !target.current(self)
                        || self.pty.lock().get_session(&target.pty_id).is_none()
                    {
                        break false;
                    }
                    let same_path = self
                        .agent_registry
                        .with_provider(&target.provider, |provider| {
                            provider.session_model_identity_path(cwd, &target.session_id)
                        })
                        .flatten()
                        .is_some_and(|current| current == path);
                    if !same_path {
                        break false;
                    }
                    if identity_bytes(&path).is_ok_and(|bytes| {
                        changed_target(&baseline, &bytes, requested, observed().as_deref())
                    }) && model_current()
                    {
                        break target.current(self);
                    }
                    let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                        break false;
                    };
                    if !signal.wait_after(generation, remaining) {
                        break false;
                    }
                },
            );
            transport::finish_command(
                result,
                injected.get(),
                || Ok(()),
                || {
                    let mux = self.mux.read();
                    if !mux.get_tab(&target.tab_id).is_some_and(|tab| {
                        target.matches(tab) && tab.launch_session_authenticated()
                    }) {
                        return Err("the session changed before input could be rearmed".into());
                    }
                    self.pty.lock().rearm_guarded_input(&target.pty_id)
                },
            )
        })();
        self.screen_state
            .lock()
            .unsubscribe_screen_changes(&target.pty_id, &signal);
        result.map_err(fail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_own_identity_confirms_only_requested_value() {
        assert!(changed_target(b"old", b"new", "target", Some("target")));
        assert!(!changed_target(b"old", b"old", "target", Some("target")));
        assert!(!changed_target(b"old", b"new", "high", Some("low")));
        assert!(!changed_target(b"old", b"new", "target", None));
    }

    #[test]
    fn own_file_events_exclude_foreign_session_files() {
        let directory = tempfile::tempdir().unwrap();
        let own = directory.path().join("summary.json");
        let foreign = directory.path().join("foreign.json");
        std::fs::write(&own, "{}").unwrap();
        std::fs::write(&foreign, "{}").unwrap();
        let expected = own.canonicalize().unwrap();
        assert!(own_file_event(&own, &expected));
        assert!(!own_file_event(&foreign, &expected));
        std::fs::remove_file(&own).unwrap();
        assert!(own_file_event(&own, &expected));
    }

    #[test]
    fn own_file_event_before_wait_is_retained() {
        let signal = crate::pty::screen_state::ScreenChangeSignal::default();
        let generation = signal.generation();
        signal.mark_changed();
        assert!(signal.wait_after(generation, Duration::ZERO));
    }
}
