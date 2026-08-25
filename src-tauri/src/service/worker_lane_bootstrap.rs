use std::path::Path;
use std::process::Command;

use crate::config::lane::{load_repo_lane_config, RepoLaneConfig};
use crate::process_util;

/// What lane bootstrap actually did: created links, skip/refusal warnings, and
/// the setup command to queue as the lane's first shell command. Recorded in
/// the lane-dispatch record; bootstrap problems never fail the dispatch.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct LaneBootstrap {
    pub(crate) links: Vec<String>,
    pub(crate) warnings: Vec<String>,
    pub(crate) setup_command: Option<String>,
}

type Linker = fn(&Path, &Path) -> Result<&'static str, String>;

pub(crate) fn bootstrap_lane(canonical_cwd: &str, lane_cwd: &str) -> LaneBootstrap {
    let config = load_repo_lane_config(Path::new(canonical_cwd));
    if config.is_empty() {
        return LaneBootstrap::default();
    }
    bootstrap_lane_with(canonical_cwd, lane_cwd, config, create_directory_link)
}

fn bootstrap_lane_with(
    canonical_cwd: &str,
    lane_cwd: &str,
    config: RepoLaneConfig,
    linker: Linker,
) -> LaneBootstrap {
    let mut bootstrap = LaneBootstrap {
        setup_command: config.setup_command,
        ..Default::default()
    };
    for name in &config.shared_directories {
        match share_directory(canonical_cwd, lane_cwd, name, linker) {
            Ok(Some(kind)) => bootstrap.links.push(format!("{name} ({kind})")),
            Ok(None) => {}
            Err(warning) => bootstrap.warnings.push(warning),
        }
    }
    bootstrap
}

/// Link one declared directory into the lane. `Ok(None)` means nothing to
/// share (absent in the primary); `Err` is a recorded warning, never fatal.
fn share_directory(
    canonical_cwd: &str,
    lane_cwd: &str,
    name: &str,
    linker: Linker,
) -> Result<Option<&'static str>, String> {
    let target = Path::new(canonical_cwd).join(name);
    match std::fs::metadata(&target) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => {
            return Err(format!(
                "shared dir '{name}' skipped: not a directory in the primary checkout"
            ))
        }
        Err(_) => return Ok(None),
    }
    let link = Path::new(lane_cwd).join(name);
    if std::fs::symlink_metadata(&link).is_ok() {
        return Err(format!(
            "shared dir '{name}' skipped: path already exists in the lane"
        ));
    }
    if !link.parent().is_some_and(Path::is_dir) {
        return Err(format!(
            "shared dir '{name}' skipped: parent directory missing in the lane"
        ));
    }
    if !git_ignores(lane_cwd, name) {
        return Err(format!(
            "shared dir '{name}' refused: name is not gitignored in the lane, \
             so a link would be stageable by git add -A"
        ));
    }
    let kind = linker(&link, &target)
        .map_err(|error| format!("shared dir '{name}' skipped: no link support ({error})"))?;
    if git_status_sees(lane_cwd, name) {
        let _ = remove_link(&link);
        return Err(format!(
            "shared dir '{name}' refused: created link was visible to git status and was removed"
        ));
    }
    Ok(Some(kind))
}

fn git_ignores(lane_cwd: &str, name: &str) -> bool {
    let mut command = Command::new("git");
    process_util::no_window_std(&mut command);
    command
        .current_dir(lane_cwd)
        .args(["check-ignore", "-q", "--", name])
        .output()
        .is_ok_and(|output| output.status.success())
}

/// True unless git provably reports nothing for the path — invisibility must be
/// proven, not assumed.
fn git_status_sees(lane_cwd: &str, name: &str) -> bool {
    let mut command = Command::new("git");
    process_util::no_window_std(&mut command);
    match command
        .current_dir(lane_cwd)
        .args(["status", "--porcelain", "--", name])
        .output()
    {
        Ok(output) if output.status.success() => {
            !String::from_utf8_lossy(&output.stdout).trim().is_empty()
        }
        _ => true,
    }
}

fn remove_link(link: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        std::fs::remove_dir(link)
    }
    #[cfg(not(windows))]
    {
        std::fs::remove_file(link)
    }
}

#[cfg(not(windows))]
fn create_directory_link(link: &Path, target: &Path) -> Result<&'static str, String> {
    std::os::unix::fs::symlink(target, link)
        .map(|_| "symlink")
        .map_err(|error| format!("symlink failed: {error}"))
}

#[cfg(windows)]
fn create_directory_link(link: &Path, target: &Path) -> Result<&'static str, String> {
    let symlink_error = match std::os::windows::fs::symlink_dir(target, link) {
        Ok(()) => return Ok("symlink"),
        Err(error) => error,
    };
    let mut command = Command::new("cmd");
    process_util::no_window_std(&mut command);
    match command
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
    {
        Ok(output) if output.status.success() => Ok("junction"),
        Ok(output) => Err(format!(
            "symlink failed: {symlink_error}; junction failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(junction_error) => Err(format!(
            "symlink failed: {symlink_error}; junction failed: {junction_error}"
        )),
    }
}

/// The setup command is simply first in the lane's shell command sequence.
/// `;` (valid in bash and PowerShell) keeps the agent launch unconditional, so
/// a failed setup stays visible as an ordinary failed first command.
pub(crate) fn compose_launch_with_setup(setup_command: Option<&str>, launch_cmd: String) -> String {
    match setup_command
        .map(str::trim)
        .filter(|setup| !setup.is_empty())
    {
        Some(setup) => format!("{setup} ; {launch_cmd}"),
        None => launch_cmd,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::worker_lane::provision_lane_for_test;
    use std::path::PathBuf;

    fn git(dir: &Path, args: &[&str]) {
        let mut command = Command::new("git");
        process_util::no_window_std(&mut command);
        let output = command.current_dir(dir).args(args).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_stdout(dir: &Path, args: &[&str]) -> String {
        let mut command = Command::new("git");
        process_util::no_window_std(&mut command);
        let output = command.current_dir(dir).args(args).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn fixture_repo(
        lane_config: Option<&str>,
        gitignore: Option<&str>,
    ) -> (tempfile::TempDir, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        git(&project, &["init", "-q"]);
        git(&project, &["config", "user.email", "test@example.com"]);
        git(&project, &["config", "user.name", "Test"]);
        std::fs::write(project.join("README.md"), "fixture").unwrap();
        if let Some(gitignore) = gitignore {
            std::fs::write(project.join(".gitignore"), gitignore).unwrap();
            git(&project, &["add", "README.md", ".gitignore"]);
        } else {
            git(&project, &["add", "README.md"]);
        }
        git(&project, &["commit", "-qm", "fixture"]);
        if let Some(lane_config) = lane_config {
            let config_dir = project.join(".codeterm");
            std::fs::create_dir_all(&config_dir).unwrap();
            std::fs::write(config_dir.join("config.toml"), lane_config).unwrap();
        }
        (root, project)
    }

    fn shared_node_modules(project: &Path) {
        std::fs::create_dir(project.join("node_modules")).unwrap();
        std::fs::write(project.join("node_modules").join("marker.txt"), "shared").unwrap();
    }

    const NODE_MODULES_CONFIG: &str =
        "[lane]\nshared_directories = [\"node_modules\"]\nsetup_command = \"npm install\"\n";

    #[test]
    fn lane_bootstrap_creates_link_resolving_to_primary_checkout() {
        let (_root, project) = fixture_repo(Some(NODE_MODULES_CONFIG), Some("node_modules\n"));
        shared_node_modules(&project);
        let lane = provision_lane_for_test(project.to_str().unwrap(), "bootstrap").unwrap();
        assert_eq!(lane.bootstrap.links.len(), 1, "{:?}", lane.bootstrap);
        assert!(lane.bootstrap.links[0].starts_with("node_modules ("));
        assert!(lane.bootstrap.warnings.is_empty(), "{:?}", lane.bootstrap);
        let through_link = PathBuf::from(&lane.cwd)
            .join("node_modules")
            .join("marker.txt");
        assert_eq!(std::fs::read_to_string(through_link).unwrap(), "shared");
        lane.cleanup().unwrap();
    }

    #[test]
    fn lane_bootstrap_link_is_invisible_to_git_status_in_the_lane() {
        let (_root, project) = fixture_repo(Some(NODE_MODULES_CONFIG), Some("node_modules\n"));
        shared_node_modules(&project);
        let lane = provision_lane_for_test(project.to_str().unwrap(), "invisible").unwrap();
        assert_eq!(lane.bootstrap.links.len(), 1, "{:?}", lane.bootstrap);
        let status = git_stdout(Path::new(&lane.cwd), &["status", "--porcelain"]);
        assert!(
            status.trim().is_empty(),
            "git add -A could stage: {status:?}"
        );
        lane.cleanup().unwrap();
    }

    #[test]
    fn lane_bootstrap_refuses_link_when_name_not_gitignored() {
        let (_root, project) = fixture_repo(Some(NODE_MODULES_CONFIG), None);
        shared_node_modules(&project);
        let lane = provision_lane_for_test(project.to_str().unwrap(), "refused").unwrap();
        assert!(lane.bootstrap.links.is_empty(), "{:?}", lane.bootstrap);
        assert_eq!(lane.bootstrap.warnings.len(), 1);
        assert!(lane.bootstrap.warnings[0].contains("not gitignored"));
        assert!(!PathBuf::from(&lane.cwd).join("node_modules").exists());
        let status = git_stdout(Path::new(&lane.cwd), &["status", "--porcelain"]);
        assert!(status.trim().is_empty(), "{status:?}");
        lane.cleanup().unwrap();
    }

    #[test]
    fn lane_bootstrap_hostile_config_proceeds_without_bootstrap() {
        let hostile = "[lane]\nshared_directories = [\"../up\", \"/abs\", \"C:/x\", \".git\"]\n";
        let (_root, project) = fixture_repo(Some(hostile), Some("node_modules\n"));
        let lane = provision_lane_for_test(project.to_str().unwrap(), "hostile").unwrap();
        assert_eq!(lane.bootstrap, LaneBootstrap::default());
        assert!(PathBuf::from(&lane.cwd).is_dir());
        lane.cleanup().unwrap();
    }

    #[test]
    fn lane_bootstrap_absent_config_keeps_lane_creation_byte_identical() {
        let (_root, project) = fixture_repo(None, None);
        let lane = provision_lane_for_test(project.to_str().unwrap(), "noconfig").unwrap();
        assert_eq!(lane.bootstrap, LaneBootstrap::default());
        let mut entries: Vec<String> = std::fs::read_dir(&lane.cwd)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        entries.sort();
        assert_eq!(entries, vec![".git", "README.md"]);
        lane.cleanup().unwrap();
    }

    #[test]
    fn lane_bootstrap_records_skip_warning_when_links_unsupported() {
        fn no_links(_link: &Path, _target: &Path) -> Result<&'static str, String> {
            Err("links unsupported in this environment".into())
        }
        let (_root, project) = fixture_repo(None, Some("node_modules\n"));
        shared_node_modules(&project);
        let lane = provision_lane_for_test(project.to_str().unwrap(), "nolink").unwrap();
        let config = RepoLaneConfig {
            shared_directories: vec!["node_modules".into()],
            setup_command: None,
        };
        let bootstrap = bootstrap_lane_with(project.to_str().unwrap(), &lane.cwd, config, no_links);
        assert!(bootstrap.links.is_empty());
        assert_eq!(bootstrap.warnings.len(), 1);
        assert!(bootstrap.warnings[0].contains("no link support"));
        assert!(!PathBuf::from(&lane.cwd).join("node_modules").exists());
        lane.cleanup().unwrap();
    }

    #[test]
    fn lane_bootstrap_setup_command_is_carried_from_config() {
        let (_root, project) = fixture_repo(Some(NODE_MODULES_CONFIG), Some("node_modules\n"));
        let lane = provision_lane_for_test(project.to_str().unwrap(), "setup").unwrap();
        assert_eq!(lane.bootstrap.setup_command.as_deref(), Some("npm install"));
        lane.cleanup().unwrap();
    }

    #[test]
    fn lane_bootstrap_setup_command_composes_as_first_shell_command() {
        assert_eq!(
            compose_launch_with_setup(Some("npm install"), "claude --task x".into()),
            "npm install ; claude --task x"
        );
        assert_eq!(compose_launch_with_setup(None, "claude".into()), "claude");
        assert_eq!(
            compose_launch_with_setup(Some("   "), "claude".into()),
            "claude"
        );
    }

    #[cfg(windows)]
    #[test]
    fn lane_bootstrap_windows_link_falls_back_to_junction_when_needed() {
        let (_root, project) = fixture_repo(Some(NODE_MODULES_CONFIG), Some("node_modules\n"));
        shared_node_modules(&project);
        let lane = provision_lane_for_test(project.to_str().unwrap(), "winlink").unwrap();
        assert_eq!(lane.bootstrap.links.len(), 1, "{:?}", lane.bootstrap);
        let kind = &lane.bootstrap.links[0];
        assert!(
            kind.ends_with("(symlink)") || kind.ends_with("(junction)"),
            "{kind}"
        );
        let through_link = PathBuf::from(&lane.cwd)
            .join("node_modules")
            .join("marker.txt");
        assert_eq!(std::fs::read_to_string(through_link).unwrap(), "shared");
        lane.cleanup().unwrap();
    }
}
