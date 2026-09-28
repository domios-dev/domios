//! Hook-independent session binding. Once a hooked provider's launch has missed
//! its binding deadline, the provider reads its own record of this tab's launch.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;

use super::ServiceContext;
use crate::agent::provider::{LaunchSessionDetection, LaunchSessionEvidence, LaunchSessionSource};
use crate::session::lifecycle::{AgentTypeWriteOrigin, BindingSource};

static LAUNCH_MARKERS: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);
static IN_FLIGHT: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);

struct DetectedLaunchSession {
    provider_id: String,
    binding_nonce: String,
    detection: LaunchSessionDetection,
}

pub(crate) fn binding_source(source: LaunchSessionSource) -> BindingSource {
    match source {
        LaunchSessionSource::PidRegistry => BindingSource::PidRegistry,
        LaunchSessionSource::LaunchMarker => BindingSource::LaunchMarker,
    }
}

fn source_label(source: BindingSource) -> &'static str {
    match source {
        BindingSource::Hook => "hook",
        BindingSource::PidRegistry => "pid_registry",
        BindingSource::LaunchMarker => "launch_marker",
    }
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

fn mint_launch_marker(tab_id: &str) -> String {
    let marker = format!("ct-launch-{}", uuid::Uuid::new_v4().simple());
    LAUNCH_MARKERS
        .lock()
        .insert(tab_id.to_string(), marker.clone());
    marker
}

pub(crate) fn clear_launch_marker(tab_id: &str) {
    LAUNCH_MARKERS.lock().remove(tab_id);
}

/// A launch command carrying the inert marker this launch registers for its
/// tab. Minted for every provider; the provider alone decides whether to use it.
pub(crate) fn marked_agent_command(
    tab_id: &str,
    agent_cmd: &str,
    params: &super::SpawnAgentParams,
    task: Option<&str>,
    injected_session_id: Option<&str>,
    registry: &crate::agent::registry::AgentRegistry,
) -> String {
    marked_agent_command_inner(
        None,
        tab_id,
        agent_cmd,
        params,
        task,
        injected_session_id,
        registry,
    )
}

pub(crate) fn marked_agent_command_for_service(
    service: &ServiceContext,
    tab_id: &str,
    agent_cmd: &str,
    params: &super::SpawnAgentParams,
    task: Option<&str>,
    injected_session_id: Option<&str>,
    registry: &crate::agent::registry::AgentRegistry,
) -> String {
    marked_agent_command_inner(
        Some(service),
        tab_id,
        agent_cmd,
        params,
        task,
        injected_session_id,
        registry,
    )
}

fn marked_agent_command_inner(
    service: Option<&ServiceContext>,
    tab_id: &str,
    agent_cmd: &str,
    params: &super::SpawnAgentParams,
    task: Option<&str>,
    injected_session_id: Option<&str>,
    registry: &crate::agent::registry::AgentRegistry,
) -> String {
    let marker = mint_launch_marker(tab_id);
    let command = match service {
        Some(service) => super::orchestrator::build_agent_command_with_launch_marker_for_service(
            service,
            tab_id,
            agent_cmd,
            params,
            task,
            injected_session_id,
            registry,
            Some(&marker),
        ),
        None => super::orchestrator::build_agent_command_with_launch_marker(
            agent_cmd,
            params,
            task,
            injected_session_id,
            registry,
            Some(&marker),
        ),
    };
    if command.trim().is_empty() {
        clear_launch_marker(tab_id);
    }
    command
}

/// Gives hand-built fresh launch params a marker minted and registered for this tab.
pub(crate) fn mark_fresh_launch(
    tab_id: &str,
    params: &mut crate::agent::provider::AgentLaunchParams,
) {
    params.launch_marker = Some(mint_launch_marker(tab_id));
}

/// The provider's stub-aware fresh launch command for hand-built params,
/// carrying this tab's marker.
pub(crate) fn marked_provider_launch_command(
    registry: &crate::agent::registry::AgentRegistry,
    provider_id: &str,
    tab_id: &str,
    cwd: Option<&str>,
    params: &mut crate::agent::provider::AgentLaunchParams,
) -> Option<String> {
    mark_fresh_launch(tab_id, params);
    let mut args = params.args.clone().unwrap_or_default();
    append_sandbox_git_metadata_args(
        registry,
        provider_id,
        params.skip_permissions,
        cwd,
        &mut args,
    );
    params.args = (!args.is_empty()).then_some(args);
    registry.with_provider(provider_id, |provider| {
        crate::agent::stub_launch::apply(provider_id, provider.build_launch_command(params))
    })
}

pub(crate) fn append_sandbox_git_metadata_args(
    registry: &crate::agent::registry::AgentRegistry,
    provider_id: &str,
    skip_permissions: bool,
    cwd: Option<&str>,
    args: &mut Vec<String>,
) {
    if skip_permissions {
        return;
    }
    let Some(flag) = registry
        .with_provider(provider_id, |provider| {
            provider.additional_writable_dir_flag()
        })
        .flatten()
    else {
        return;
    };
    let Some(cwd) = cwd else { return };
    let Some(paths) = git_worktree_metadata_dirs(cwd) else {
        return;
    };
    for path in paths {
        args.push(flag.clone());
        args.push(path.to_string_lossy().into_owned());
    }
}

fn git_worktree_metadata_dirs(cwd: &str) -> Option<[std::path::PathBuf; 2]> {
    let output = std::process::Command::new("git")
        .args(["-C", cwd, "rev-parse", "--git-dir", "--git-common-dir"])
        .output();
    let Ok(output) = output else { return None };
    if !output.status.success() {
        return None;
    }
    let Ok(stdout) = String::from_utf8(output.stdout) else {
        return None;
    };
    let mut lines = stdout.lines();
    let (Some(git_dir), Some(common_dir)) = (lines.next(), lines.next()) else {
        return None;
    };
    let cwd = std::path::Path::new(cwd);
    let resolve = |path: &str| {
        let path = std::path::Path::new(path);
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            cwd.join(path)
        };
        path.canonicalize().ok()
    };
    let (Some(git_dir), Some(common_dir)) = (resolve(git_dir), resolve(common_dir)) else {
        return None;
    };
    if git_dir == common_dir || !git_dir.join("commondir").is_file() {
        return None;
    }
    Some([git_dir, common_dir])
}

#[cfg(test)]
pub(crate) fn registered_launch_marker(tab_id: &str) -> Option<String> {
    LAUNCH_MARKERS.lock().get(tab_id).cloned()
}

impl ServiceContext {
    /// One detection per tab at a time. Detection itself refuses a bound tab,
    /// and the commit refuses a launch the tab has since rotated.
    pub(crate) fn detect_unbound_launch_session(&self, tab_id: &str) {
        if !IN_FLIGHT.lock().insert(tab_id.to_string()) {
            return;
        }
        let svc = self.clone();
        let tab_id = tab_id.to_string();
        tauri::async_runtime::spawn(async move {
            let probe_svc = svc.clone();
            let probe_tab = tab_id.clone();
            let detected =
                tokio::task::spawn_blocking(move || probe_svc.probe_launch_session(&probe_tab))
                    .await
                    .ok()
                    .flatten();
            if let Some(detected) = detected {
                svc.commit_detected_launch_session(&tab_id, detected).await;
            }
            IN_FLIGHT.lock().remove(&tab_id);
        });
    }

    fn probe_launch_session(&self, tab_id: &str) -> Option<DetectedLaunchSession> {
        let (provider_id, pty_id, binding_nonce, cwd, launched_at_ms) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id)?;
            if tab.agent_session_id.is_some() {
                return None;
            }
            let launched_at_ms = tab
                .agent_set_at
                .map(|set_at| epoch_ms().saturating_sub(set_at.elapsed().as_millis() as u64))
                .unwrap_or(0);
            (
                tab.agent_type.clone()?,
                tab.pty_id.clone()?,
                tab.session_binding_nonce.clone()?,
                tab.cwd.clone().unwrap_or_default(),
                launched_at_ms,
            )
        };
        let root_pid = {
            let pty = self.pty.lock();
            let session = pty.get_session(&pty_id)?;
            let pid = session.child.lock().ok()?.process_id();
            pid?
        };
        let evidence = LaunchSessionEvidence {
            cwd,
            launch_marker: LAUNCH_MARKERS.lock().get(tab_id).cloned(),
            launched_at_ms,
            processes: super::process_tree::launch_processes(root_pid),
        };
        let detection = self
            .agent_registry
            .with_provider(&provider_id, |provider| {
                provider.detect_launch_session(&evidence)
            })
            .flatten()?;
        Some(DetectedLaunchSession {
            provider_id,
            binding_nonce,
            detection,
        })
    }

    async fn commit_detected_launch_session(&self, tab_id: &str, detected: DetectedLaunchSession) {
        let DetectedLaunchSession {
            provider_id,
            binding_nonce,
            detection,
        } = detected;
        let source = binding_source(detection.source);
        let session_id = detection.session_id;
        let claim = match crate::api::hook_routes::bind_detected_session(
            &self.mux,
            self.agent_registry.as_ref(),
            tab_id,
            &provider_id,
            &session_id,
            &binding_nonce,
        ) {
            Ok(claim) => claim,
            Err((status, message, _)) => {
                log::info!(
                    "[launch-session-detection] Tab {tab_id} refused {provider_id} session {session_id} from {} ({status}): {message}",
                    source_label(source),
                );
                return;
            }
        };
        log::info!(
            "[launch-session-detection] Tab {tab_id} bound {provider_id} session {session_id} from {}",
            source_label(source),
        );
        LAUNCH_MARKERS.lock().remove(tab_id);
        if let Err((status, message)) = crate::api::hook_routes::finish_session_binding(
            self,
            tab_id,
            &provider_id,
            &session_id,
            Some(&binding_nonce),
            Some(AgentTypeWriteOrigin::Hook),
            source_label(source),
            &serde_json::Value::Null,
            claim,
            source,
            false,
        )
        .await
        {
            log::warn!(
                "[launch-session-detection] Tab {tab_id} binding follow-up failed ({status}): {message}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(cwd: &std::path::Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .current_dir(cwd)
            .args(args)
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn lane_git_writable_roots_derived_from_real_worktree() {
        let temp = tempfile::tempdir().expect("temporary repository");
        let repo = temp.path().join("repo");
        let lane = temp.path().join("lane");
        std::fs::create_dir(&repo).expect("repository directory");
        git(&repo, &["init"]);
        git(
            &repo,
            &[
                "-c",
                "user.name=CodeTerm",
                "-c",
                "user.email=codeterm@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "initial",
            ],
        );
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-b",
                "lane/worker",
                lane.to_str().unwrap(),
            ],
        );

        let roots = git_worktree_metadata_dirs(lane.to_str().unwrap()).expect("linked worktree");
        let output = std::process::Command::new("git")
            .current_dir(&lane)
            .args(["rev-parse", "--git-dir"])
            .output()
            .expect("git reports lane gitdir");
        let lane_gitdir = String::from_utf8(output.stdout).expect("path is UTF-8");
        let lane_gitdir = std::path::Path::new(lane_gitdir.trim());
        let lane_gitdir = if lane_gitdir.is_absolute() {
            lane_gitdir.to_path_buf()
        } else {
            lane.join(lane_gitdir)
        };
        let expected_lane_gitdir = lane_gitdir.canonicalize().expect("lane gitdir resolves");
        assert_eq!(roots[0], expected_lane_gitdir);
        assert!(roots[0].join("commondir").is_file());
        assert_ne!(roots[0], repo);
        assert_ne!(roots[1], repo);
        assert!(roots[1].ends_with(".git"));
        assert!(roots[0].starts_with(&roots[1]));
    }

    #[test]
    fn ordinary_repository_has_no_additional_git_metadata_roots() {
        let temp = tempfile::tempdir().expect("temporary repository");
        git(temp.path(), &["init"]);
        assert!(git_worktree_metadata_dirs(temp.path().to_str().unwrap()).is_none());
    }

    #[test]
    fn sandboxed_lane_launch_includes_git_roots_only_for_lanes() {
        let temp = tempfile::tempdir().expect("temporary repository");
        let repo = temp.path().join("repo");
        let lane = temp.path().join("lane");
        std::fs::create_dir(&repo).expect("repository directory");
        git(&repo, &["init"]);
        git(
            &repo,
            &[
                "-c",
                "user.name=CodeTerm",
                "-c",
                "user.email=codeterm@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "initial",
            ],
        );
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-b",
                "lane/worker",
                lane.to_str().unwrap(),
            ],
        );
        let registry = crate::agent::registry::AgentRegistry::new();
        registry.register_provider(Box::new(
            crate::agent::test_provider::MockProvider::new("fixture")
                .with_additional_writable_dir_flag("--add-dir"),
        ));

        let roots = git_worktree_metadata_dirs(lane.to_str().unwrap()).expect("linked worktree");
        let mut args = Vec::new();
        append_sandbox_git_metadata_args(
            &registry,
            "fixture",
            false,
            Some(lane.to_str().unwrap()),
            &mut args,
        );
        assert_eq!(
            args,
            vec![
                "--add-dir".to_string(),
                roots[0].to_string_lossy().into_owned(),
                "--add-dir".to_string(),
                roots[1].to_string_lossy().into_owned(),
            ]
        );
        assert!(args.iter().all(|arg| arg != repo.to_str().unwrap()));
        assert!(args.iter().all(|arg| arg != lane.to_str().unwrap()));

        let mut full_access_args = vec!["--existing".to_string()];
        append_sandbox_git_metadata_args(
            &registry,
            "fixture",
            true,
            Some(lane.to_str().unwrap()),
            &mut full_access_args,
        );
        assert_eq!(full_access_args, vec!["--existing".to_string()]);

        let ordinary = temp.path().join("ordinary");
        std::fs::create_dir(&ordinary).expect("ordinary repository directory");
        git(&ordinary, &["init"]);
        let mut ordinary_args = vec!["--existing".to_string()];
        append_sandbox_git_metadata_args(
            &registry,
            "fixture",
            false,
            Some(ordinary.to_str().unwrap()),
            &mut ordinary_args,
        );
        assert_eq!(ordinary_args, vec!["--existing".to_string()]);
    }

    #[test]
    fn detection_sources_journal_as_their_own_binding_source() {
        assert_eq!(
            binding_source(LaunchSessionSource::PidRegistry),
            BindingSource::PidRegistry
        );
        assert_eq!(
            binding_source(LaunchSessionSource::LaunchMarker),
            BindingSource::LaunchMarker
        );
        assert_ne!(
            binding_source(LaunchSessionSource::PidRegistry),
            BindingSource::Hook
        );
    }

    #[test]
    fn marked_agent_command_carries_the_marker_its_tab_currently_holds() {
        let registry = crate::agent::registry::AgentRegistry::new();
        registry.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
            "marked",
        )));
        let params = crate::service::SpawnAgentParams {
            agent: Some("marked".into()),
            ..Default::default()
        };

        let first = marked_agent_command("tab-marked", "marked", &params, None, None, &registry);
        let first_marker = registered_launch_marker("tab-marked").expect("first marker");
        assert!(!first_marker.is_empty());
        assert_eq!(first, format!("marked-cli --launch-marker {first_marker}"));

        let relaunch = marked_agent_command("tab-marked", "marked", &params, None, None, &registry);
        let relaunch_marker = registered_launch_marker("tab-marked").expect("relaunch marker");
        assert_ne!(relaunch_marker, first_marker);
        assert_eq!(
            relaunch,
            format!("marked-cli --launch-marker {relaunch_marker}")
        );
    }

    #[test]
    fn marked_provider_launch_command_hands_every_fresh_launch_its_tabs_marker() {
        use crate::agent::provider::AgentLaunchParams;

        let registry = crate::agent::registry::AgentRegistry::new();
        registry.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
            "direct",
        )));
        let cases = [
            ("tab-direct-plain", AgentLaunchParams::default()),
            (
                "tab-direct-task",
                AgentLaunchParams {
                    task: Some("fixture task".into()),
                    ..Default::default()
                },
            ),
            (
                "tab-direct-injected",
                AgentLaunchParams {
                    session_id: Some("injected-session".into()),
                    ..Default::default()
                },
            ),
        ];

        for (tab_id, mut params) in cases {
            let command =
                marked_provider_launch_command(&registry, "direct", tab_id, None, &mut params)
                    .expect("registered provider builds a command");
            let marker = registered_launch_marker(tab_id).expect("tab registers its marker");
            assert_eq!(params.launch_marker.as_deref(), Some(marker.as_str()));
            assert_eq!(command, format!("direct-cli --launch-marker {marker}"));
        }
    }

    #[test]
    fn mark_fresh_launch_registers_the_marker_it_hands_the_params() {
        let mut params = crate::agent::provider::AgentLaunchParams::default();
        mark_fresh_launch("tab-mark-only", &mut params);
        let marker = registered_launch_marker("tab-mark-only").expect("registered marker");
        assert!(!marker.is_empty());
        assert_eq!(params.launch_marker, Some(marker));
    }
}
