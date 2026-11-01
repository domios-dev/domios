#[path = "support/codeterm_bin.rs"]
mod codeterm_bin;
#[path = "support/json_output.rs"]
mod json_output;

use std::process::Command;

#[test]
fn blueprint_v2_project_offline_list_view_and_refusals() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join(".domios/blueprints");
    std::fs::create_dir_all(&directory).unwrap();
    let text = |name: &str| {
        format!("schema_version: 2\nkind: pipeline\nname: {name}\nintent: inspect source without execution\nsteps: [{{id: emit, run: {{lit: 'printf ready'}}}}]\n")
    };
    std::fs::write(directory.join("valid.yaml"), text("valid")).unwrap();
    std::fs::write(directory.join("mismatch.yaml"), text("other")).unwrap();
    let call = |args: &[&str]| {
        let mut command = Command::new(codeterm_bin::codeterm_bin());
        command
            .current_dir(root.path())
            .args(["--json", "blueprint", "project"])
            .args(args);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        command.output().unwrap()
    };
    let list = call(&["list"]);
    assert!(
        list.status.success(),
        "{}",
        String::from_utf8_lossy(&list.stderr)
    );
    let rows = json_output::result(&list.stdout);
    assert_eq!(rows.as_array().unwrap().len(), 2);
    assert!(rows
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["name"] == "valid"
            && row["path"] == ".domios/blueprints/valid.yaml"
            && row["intent"] == "inspect source without execution"));
    let shown = call(&["show", ".domios/blueprints/valid.yaml"]);
    assert!(shown.status.success());
    let shown = json_output::result(&shown.stdout);
    assert_eq!(shown["document"]["name"], "valid");
    for path in [
        ".domios/blueprints/mismatch.yaml",
        "../escape.yaml",
        ".domios/blueprints/../escape.yaml",
    ] {
        assert!(!call(&["show", path]).status.success());
    }
    assert!(!call(&["run", ".domios/blueprints/valid.yaml"])
        .status
        .success());
    assert!(!call(&["apply", ".domios/blueprints/valid.yaml"])
        .status
        .success());
    assert_eq!(
        std::fs::read_to_string(directory.join("valid.yaml")).unwrap(),
        text("valid")
    );
}
