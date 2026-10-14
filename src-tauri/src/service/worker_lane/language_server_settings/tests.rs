use super::*;
use crate::service::worker_lane::{git_value, provision_lane_for_test, run_git};

fn fixture() -> (tempfile::TempDir, String) {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let cwd = project.to_str().unwrap().to_string();
    run_git(&cwd, ["init", "-q"]).unwrap();
    run_git(&cwd, ["config", "user.email", "test@example.com"]).unwrap();
    run_git(&cwd, ["config", "user.name", "Test"]).unwrap();
    std::fs::write(project.join("README.md"), "fixture").unwrap();
    (root, cwd)
}

fn commit(cwd: &str) {
    run_git(cwd, ["add", "-A"]).unwrap();
    run_git(cwd, ["commit", "-qm", "fixture"]).unwrap();
}

fn read_json(cwd: &str, relative: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(Path::new(cwd).join(relative)).unwrap()).unwrap()
}

#[test]
fn provisioning_disables_lane_builds_without_dirt_or_canonical_settings() {
    let (_root, cwd) = fixture();
    commit(&cwd);
    let lane = provision_lane_for_test(&cwd, "no-build").unwrap();
    for relative in SETTINGS_FILES {
        assert!(Path::new(&lane.cwd).join(relative).is_file());
        assert!(!Path::new(&cwd).join(relative).exists());
    }
    for relative in &SETTINGS_FILES[..2] {
        assert_eq!(
            read_json(&lane.cwd, relative)["enabledPlugins"]
                ["rust-analyzer-lsp@claude-plugins-official"],
            false
        );
    }
    let vscode = read_json(&lane.cwd, SETTINGS_FILES[2]);
    assert_eq!(vscode["rust-analyzer.checkOnSave"], false);
    assert_eq!(vscode["rust-analyzer.cargo.buildScripts.enable"], false);
    assert_eq!(
        vscode["rust-analyzer.cargo.buildScripts.rebuildOnSave"],
        false
    );
    assert_eq!(vscode["rust-analyzer.procMacro.enable"], false);
    let analyzer: toml::Value = toml::from_str(
        &std::fs::read_to_string(Path::new(&lane.cwd).join(SETTINGS_FILES[3])).unwrap(),
    )
    .unwrap();
    assert_eq!(analyzer["checkOnSave"].as_bool(), Some(false));
    assert_eq!(
        analyzer["cargo"]["buildScripts"]["enable"].as_bool(),
        Some(false)
    );
    assert_eq!(
        analyzer["cargo"]["buildScripts"]["overrideCommand"][0].as_str(),
        Some("git")
    );
    assert_eq!(
        analyzer["cargo"]["buildScripts"]["overrideCommand"][1].as_str(),
        Some("--version")
    );
    assert!(git_value(&lane.cwd, ["status", "--porcelain"])
        .unwrap()
        .is_empty());
    run_git(&lane.cwd, ["add", "-A"]).unwrap();
    assert!(git_value(&lane.cwd, ["diff", "--cached", "--name-only"])
        .unwrap()
        .is_empty());
    assert!(!Path::new(&cwd).join(".gitignore").exists());
    lane.cleanup().unwrap();
}

#[test]
fn inherited_settings_are_preserved_and_lane_overrides_never_stage() {
    let (_root, cwd) = fixture();
    let files = [
        (
            ".claude/settings.json",
            r#"{"enabledPlugins":{"other@market":true,"rust-analyzer-lsp@claude-plugins-official":true}}"#,
        ),
        (
            ".claude/settings.local.json",
            r#"{"permissions":{"allow":["Read"]}}"#,
        ),
        (
            ".vscode/settings.json",
            r#"{"editor.fontSize":14,"rust-analyzer.checkOnSave":true}"#,
        ),
        (
            "rust-analyzer.toml",
            "checkOnSave = true\n[cargo]\nallTargets = false\n",
        ),
        (".gitignore", "node_modules/\n"),
    ];
    for (relative, content) in files {
        let path = Path::new(&cwd).join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }
    commit(&cwd);
    let lane = provision_lane_for_test(&cwd, "inherited").unwrap();
    assert_eq!(
        read_json(&lane.cwd, SETTINGS_FILES[0])["enabledPlugins"]["other@market"],
        true
    );
    assert_eq!(
        read_json(&lane.cwd, SETTINGS_FILES[1])["permissions"]["allow"],
        json!(["Read"])
    );
    assert_eq!(
        read_json(&lane.cwd, SETTINGS_FILES[2])["editor.fontSize"],
        14
    );
    for (relative, content) in files {
        assert_eq!(
            std::fs::read_to_string(Path::new(&cwd).join(relative)).unwrap(),
            content
        );
    }
    run_git(&lane.cwd, ["add", "-A"]).unwrap();
    assert!(git_value(&lane.cwd, ["status", "--porcelain"])
        .unwrap()
        .is_empty());
    lane.cleanup().unwrap();
}

#[test]
fn invalid_settings_refuse_provisioning_and_remove_the_lane() {
    let (root, cwd) = fixture();
    std::fs::create_dir(Path::new(&cwd).join(".vscode")).unwrap();
    std::fs::write(Path::new(&cwd).join(".vscode/settings.json"), "invalid").unwrap();
    commit(&cwd);
    assert!(provision_lane_for_test(&cwd, "invalid").is_err());
    assert_eq!(
        std::fs::read_dir(root.path().join("project-worktrees"))
            .unwrap()
            .count(),
        0
    );
    assert!(git_value(&cwd, ["branch", "--list", "lane/*"])
        .unwrap()
        .is_empty());
}

#[cfg(unix)]
#[test]
fn symlinked_settings_refuse_provisioning_without_writing_the_target() {
    let (_root, cwd) = fixture();
    let target = tempfile::tempdir().unwrap();
    let settings = target.path().join("settings.json");
    std::fs::write(&settings, "{}").unwrap();
    std::os::unix::fs::symlink(target.path(), Path::new(&cwd).join(".claude")).unwrap();
    commit(&cwd);
    assert!(provision_lane_for_test(&cwd, "symlink")
        .unwrap_err()
        .contains("symlink"));
    assert_eq!(std::fs::read_to_string(settings).unwrap(), "{}");
}
