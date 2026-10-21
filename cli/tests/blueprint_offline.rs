#[path = "support/codeterm_bin.rs"]
mod codeterm_bin;

#[test]
fn blueprint_check_new_and_schema_are_offline_and_check_json_has_positions() {
    let binary = codeterm_bin::codeterm_bin();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("bad.yaml");
    std::fs::write(&path,"schema_version: 1\nname: Watch\nwatch:\n  probe: echo ready\n  question: State?\n  buckets: [ready]\n  every: tomorrow\n").unwrap();
    for args in [
        vec!["blueprint", "schema", "--json"],
        vec!["blueprint", "new", "command", "--json"],
        vec!["blueprint", "check", path.to_str().unwrap(), "--json"],
    ] {
        let output = std::process::Command::new(&binary)
            .args(&args)
            .env(
                "CODETERM_SOCKET_PATH",
                format!("absent-{}", uuid::Uuid::new_v4()),
            )
            .env("CODETERM_API_URL", "http://127.0.0.1:1")
            .env_remove("CODETERM_API_TOKEN")
            .output()
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&output.stderr)));
        if args[1] == "check" {
            assert_eq!(output.status.code(), Some(2));
            assert_eq!(value["ok"], false);
            assert!(value["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["field"] == "watch.every" && d["line"] == 7 && d["column"] == 3));
        } else {
            assert!(output.status.success(), "{value}");
        }
    }
}
