use std::collections::HashSet;
use std::path::PathBuf;

use super::ServiceContext;

pub(crate) fn capture<T>(
    service: &ServiceContext,
    tab_id: &str,
    operation: impl FnOnce() -> T,
) -> T {
    let root = service.agent_pad_data_dir.join("runtime/task-files");
    let previous = codeterm_util::task_files::owned_paths(&root, tab_id);
    let (result, mut paths) = codeterm_util::task_files::capture_in(&root, tab_id, operation);
    paths.extend(previous);
    if !paths.is_empty() {
        let mut registry = service.launch_task_files.lock();
        let owned = registry.entry(tab_id.to_string()).or_default();
        for path in paths {
            if !owned.contains(&path) {
                owned.push(path);
            }
        }
    }
    result
}

pub(crate) fn retire_settings_for_launch(service: &ServiceContext, tab_id: &str, command: &str) {
    let paths: Vec<_> = service
        .launch_task_files
        .lock()
        .get(tab_id)
        .into_iter()
        .flatten()
        .filter(|path| superseded_settings(path, command))
        .cloned()
        .collect();
    let root = service.agent_pad_data_dir.join("runtime/task-files");
    let removed = codeterm_util::task_files::release_paths(&root, tab_id, &paths);
    if let Some(paths) = service.launch_task_files.lock().get_mut(tab_id) {
        paths.retain(|path| !removed.contains(path));
    }
}

fn superseded_settings(path: &std::path::Path, command: &str) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".ct-settings.json") && !command.contains(name))
}

pub(crate) fn release_tab(service: &ServiceContext, tab_id: &str) -> Vec<PathBuf> {
    // Live-tab cleanup can race another launch's task-file read.
    if service.mux.read().get_tab(tab_id).is_some() {
        return Vec::new();
    }
    let paths = service
        .launch_task_files
        .lock()
        .remove(tab_id)
        .unwrap_or_default();
    let root = service
        .agent_pad_data_dir
        .join("runtime")
        .join("task-files");
    codeterm_util::task_files::release_tab(&root, tab_id, &paths)
}

pub(crate) fn sweep_after_restore(service: &ServiceContext) -> Vec<PathBuf> {
    let live_owners: HashSet<String> = service.mux.read().snapshot().tabs.into_keys().collect();
    let root = service
        .agent_pad_data_dir
        .join("runtime")
        .join("task-files");
    let removed = codeterm_util::task_files::sweep(&root, &live_owners);
    let mut registry = service.launch_task_files.lock();
    registry.retain(|tab_id, paths| {
        if live_owners.contains(tab_id) {
            paths.retain(|path| path.exists());
            true
        } else {
            false
        }
    });
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::TabCloseReason;
    use crate::service::test_fixture::ServiceFixture;

    fn fixture_with_task_file() -> (ServiceFixture, String, PathBuf) {
        let (mux, tab_id) = MultiplexerState::new();
        let fixture = ServiceFixture::new(mux);
        let root = fixture
            .service
            .agent_pad_data_dir
            .join("runtime")
            .join("task-files");
        let marker =
            codeterm_util::task_files::instance_marker_for(&fixture.service.agent_pad_data_dir);
        let path = capture(&fixture.service, &tab_id, || {
            codeterm_util::task_files::write_at(
                &root,
                &marker,
                &tab_id,
                "task payload",
                Some(".ct-task.md"),
            )
            .expect("write task file")
        });
        (fixture, tab_id, path)
    }

    #[test]
    #[cfg_attr(
        not(windows),
        ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
    )]
    fn task_files_deleted_on_tab_close_any_reason() {
        for reason in [
            TabCloseReason::User,
            TabCloseReason::PolicyWorker,
            TabCloseReason::PolicyStartup,
            TabCloseReason::Watchdog,
            TabCloseReason::PtyExit,
            TabCloseReason::SpawnRollback,
            TabCloseReason::AgentRunEnded,
            TabCloseReason::Cancelled,
            TabCloseReason::WorkspaceClosed,
        ] {
            let (fixture, tab_id, path) = fixture_with_task_file();
            fixture
                .service
                .close_tab_with_reason(&tab_id, reason)
                .expect("close tab");
            assert!(
                !path.exists(),
                "task file survived {} close",
                reason.label()
            );
        }
    }

    #[test]
    #[cfg_attr(
        not(windows),
        ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
    )]
    fn task_files_deleted_on_launch_failure() {
        let (fixture, tab_id, path) = fixture_with_task_file();
        fixture
            .service
            .close_tab_with_reason(&tab_id, TabCloseReason::SpawnRollback)
            .expect("roll back failed launch");
        assert!(!path.exists());
    }

    #[test]
    #[cfg_attr(
        not(windows),
        ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
    )]
    fn active_launch_cleanup_preserves_task_files_until_tab_close() {
        let (fixture, tab_id, first) = fixture_with_task_file();
        let root = fixture
            .service
            .agent_pad_data_dir
            .join("runtime")
            .join("task-files");
        let marker =
            codeterm_util::task_files::instance_marker_for(&fixture.service.agent_pad_data_dir);
        let second = capture(&fixture.service, &tab_id, || {
            codeterm_util::task_files::write_at(
                &root,
                &marker,
                &tab_id,
                "concurrent launch payload",
                Some(".ct-task.md"),
            )
            .expect("write concurrent launch task file")
        });

        assert!(release_tab(&fixture.service, &tab_id).is_empty());
        assert!(first.exists());
        assert!(second.exists());
    }

    #[test]
    fn task_file_sweep_is_instance_scoped() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("runtime").join("task-files");
        let marker = codeterm_util::task_files::instance_marker_for(temp.path());
        let live = codeterm_util::task_files::write_at(
            &root,
            &marker,
            "live-tab",
            "live",
            Some(".ct-task.md"),
        )
        .unwrap();
        let orphan = codeterm_util::task_files::write_at(
            &root,
            &marker,
            "orphan-tab",
            "orphan",
            Some(".ct-task.md"),
        )
        .unwrap();
        let foreign = root.join("codeterm-plugin-other-instance-666f726569676e-abcd.ct-task.md");
        std::fs::write(&foreign, "foreign").unwrap();

        assert_eq!(
            codeterm_util::task_files::sweep(&root, &HashSet::from(["live-tab".into()])),
            vec![orphan]
        );
        assert!(live.exists());
        assert!(foreign.exists());
    }
}

#[cfg(test)]
mod settings_tests {
    use super::*;
    #[test]
    fn claude_settings_retirement_preserves_current_launch_and_task_files() {
        let old = PathBuf::from("/data/runtime/task-files/old.ct-settings.json");
        let current = PathBuf::from("/data/runtime/task-files/new.ct-settings.json");
        let command = "claude --settings '/data/runtime/task-files/new.ct-settings.json'";
        assert!(superseded_settings(&old, command));
        assert!(!superseded_settings(&current, command));
        assert!(!superseded_settings(
            std::path::Path::new("payload.ct-task.md"),
            command
        ));
    }
}
