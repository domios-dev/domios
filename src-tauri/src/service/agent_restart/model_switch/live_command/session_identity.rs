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
    path == expected
        || path
            .canonicalize()
            .ok()
            .zip(expected.canonicalize().ok())
            .is_some_and(|(path, expected)| path == expected)
        || (path.file_name() == expected.file_name()
            && path
                .parent()
                .and_then(|parent| parent.canonicalize().ok())
                .zip(
                    expected
                        .parent()
                        .and_then(|parent| parent.canonicalize().ok()),
                )
                .is_some_and(|(parent, expected)| parent == expected))
}

fn identity_path_retained(current: Option<&Path>, expected: &Path) -> bool {
    current.is_none_or(|current| own_file_event(current, expected))
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
        deadline: Instant,
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
        let idle = || {
            target.current(self)
                && self
                    .mux
                    .read()
                    .get_tab(&target.tab_id)
                    .is_some_and(|tab| tab.pending_prompt.is_none())
                && self.pty.lock().get_session(&target.pty_id).is_some()
                && self.command_tab_idle(target)
        };
        if !idle() {
            return Err(fail("the agent is busy or waiting on a prompt".into()));
        }
        if !model_current() {
            return Err(fail(
                "the session model does not match the requested effort's model".into(),
            ));
        }
        if effort.is_some() && observed().as_deref() == Some(requested) {
            return Ok(());
        }
        let signal = command_signal(&mut self.screen_state.lock(), &target.pty_id);
        crate::service::boot_wait::observe_boot_changes(&signal);
        let started = Instant::now();
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
            let failure = std::cell::RefCell::new(None::<String>);
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
                    )?;
                    log::info!("[agent-model/timing] tab={} source=session_identity phase=injected effort={} elapsed_ms={}", target.tab_id, effort.is_some(), started.elapsed().as_millis());
                    Ok(())
                },
                || loop {
                    let generation = signal.generation();
                    if !target.current(self)
                        || self.pty.lock().get_session(&target.pty_id).is_none()
                    {
                        log::warn!(
                            "[agent-model] identity wait lost session authority: tab={}",
                            target.tab_id
                        );
                        *failure.borrow_mut() = Some("the command was sent but its session authority changed before confirmation".into());
                        break false;
                    }
                    let same_path = self
                        .agent_registry
                        .with_provider(&target.provider, |provider| {
                            provider.session_model_identity_path(cwd, &target.session_id)
                        })
                        .flatten()
                        .map_or(true, |current| {
                            identity_path_retained(Some(&current), &path)
                        });
                    if !same_path {
                        log::warn!(
                            "[agent-model] identity wait changed file: tab={}",
                            target.tab_id
                        );
                        *failure.borrow_mut() = Some("the command was sent but its own-session identity file changed before confirmation".into());
                        break false;
                    }
                    if identity_bytes(&path).is_ok_and(|bytes| {
                        changed_target(&baseline, &bytes, requested, observed().as_deref())
                    }) && model_current()
                    {
                        log::info!("[agent-model/timing] tab={} source=session_identity phase=identity_confirmed effort={} elapsed_ms={}", target.tab_id, effort.is_some(), started.elapsed().as_millis());
                        break target.current(self);
                    }
                    let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                        break false;
                    };
                    if !signal.wait_after(generation, remaining) {
                        log::warn!(
                            "[agent-model] identity wait ended: tab={} remaining_ms={}",
                            target.tab_id,
                            deadline
                                .saturating_duration_since(Instant::now())
                                .as_millis()
                        );
                        if Instant::now() < deadline {
                            *failure.borrow_mut() = Some("the command was sent but its confirmation event stream closed before the deadline".into());
                        }
                        break false;
                    }
                },
            );
            let result = result.map_err(|error| failure.into_inner().unwrap_or(error));
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
        if let Err(error) = &result {
            log::warn!("[agent-model/timing] tab={} source=session_identity phase=refused elapsed_ms={} error={error}", target.tab_id, started.elapsed().as_millis());
        }
        result.map_err(fail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_target_summary_cannot_skip_a_model_command() {
        use crate::agent::provider::ModelSwitchConfirmationSource as Source;
        assert!(model_command_required(Source::SessionIdentity, true, false));
        assert!(model_command_required(Source::SessionIdentity, false, true));
        assert!(!model_command_required(Source::SessionIdentity, true, true));
        assert!(!model_command_required(Source::Transcript, true, false));
    }

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
        assert!(own_file_event(&expected, &own));
        assert!(!own_file_event(&foreign, &expected));
        std::fs::remove_file(&own).unwrap();
        assert!(own_file_event(&own, &expected));
        assert!(own_file_event(&own, &own));
        assert!(own_file_event(&expected, &own));
    }

    #[test]
    fn transient_identity_absence_keeps_waiting_but_foreign_path_refuses() {
        let own = Path::new("owned-session/summary.json");
        assert!(identity_path_retained(None, own));
        assert!(identity_path_retained(Some(own), own));
        assert!(!identity_path_retained(
            Some(Path::new("foreign-session/summary.json")),
            own
        ));
        assert!(!changed_target(b"old", b"partial", "chosen", None));
        assert!(changed_target(
            b"old",
            b"complete",
            "chosen",
            Some("chosen")
        ));
    }

    #[test]
    fn picker_model_then_effort_and_next_apply_have_independent_waits() {
        let mut screens = crate::pty::screen_state::ScreenState::new();
        let shared = screens.arm_screen_change_signal("owned-pty");
        for _ in 0..3 {
            let signal = command_signal(&mut screens, "owned-pty");
            let generation = signal.generation();
            signal.mark_changed();
            assert!(signal.wait_after(generation, Duration::ZERO));
            assert!(signal.wait_until_changed(generation));
            screens.unsubscribe_screen_changes("owned-pty", &signal);
            let generation = shared.generation();
            shared.mark_changed();
            assert!(shared.wait_after(generation, Duration::ZERO));
            assert!(shared.wait_until_changed(generation));
        }
        assert_eq!(screens.screen_change_subscriber_count(), 0);
    }

    #[test]
    fn own_file_event_before_wait_is_retained() {
        let signal = crate::pty::screen_state::ScreenChangeSignal::default();
        let generation = signal.generation();
        signal.mark_changed();
        assert!(signal.wait_after(generation, Duration::ZERO));
    }
}
