#[path = "../../build/plugin_bundles.rs"]
mod plugin_bundles;

fn main() {
    let app = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("Resolve the app manifest directory for bundled resources");
    let version_source = app.join("tauri.conf.json");
    println!("cargo:rerun-if-changed={}", version_source.display());
    let config: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&version_source).expect("Read release version configuration"),
    )
    .expect("Parse release version configuration");
    let version = config["version"]
        .as_str()
        .expect("Release version must be a string");
    assert!(
        !version.is_empty()
            && version
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b".-+".contains(&byte)),
        "Invalid release version"
    );
    println!("cargo:rustc-env=DOMIOS_VERSION={version}");
    plugin_bundles::generate_system_plugin_hashes(&app);
    for (key, args) in [
        ("BUILD_SHA", vec!["rev-parse", "HEAD"]),
        ("BUILD_HASH", vec!["rev-parse", "--short", "HEAD"]),
    ] {
        let override_key = format!("CODETERM_{key}");
        println!("cargo:rerun-if-env-changed={override_key}");
        let value = std::env::var(&override_key).unwrap_or_else(|_| {
            let output = std::process::Command::new("git")
                .args(args)
                .output()
                .expect("git build identity");
            assert!(output.status.success());
            String::from_utf8(output.stdout)
                .expect("UTF-8 git identity")
                .trim()
                .to_string()
        });
        assert!(
            value.len() >= 7 && value.len() <= 64 && value.bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid server build identity"
        );
        println!("cargo:rustc-env={key}={value}");
    }
    println!("cargo:rerun-if-env-changed=CODETERM_SERVER_DEV_FRONTEND_ORIGIN");
    let frontend = std::env::var("CODETERM_SERVER_DEV_FRONTEND_ORIGIN").unwrap_or_default();
    assert!(
        frontend.bytes().all(|byte| byte.is_ascii_graphic()),
        "Invalid development frontend origin"
    );
    println!("cargo:rustc-env=SERVER_DEV_FRONTEND_ORIGIN={frontend}");
    println!("cargo:rerun-if-env-changed=CODETERM_SERVER_DEV_IDENTIFIER");
    let identifier = std::env::var("CODETERM_SERVER_DEV_IDENTIFIER").unwrap_or_default();
    assert!(identifier
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b".-".contains(&byte)));
    println!("cargo:rustc-env=SERVER_DEV_IDENTIFIER={identifier}");
    println!("cargo:rerun-if-env-changed=CODETERM_SERVER_CLI_SHA256");
    let worker = std::env::var("CODETERM_SERVER_CLI_SHA256").unwrap_or_default();
    assert!(
        worker.is_empty() || worker.len() == 64 && worker.bytes().all(|b| b.is_ascii_hexdigit())
    );
    println!("cargo:rustc-env=SERVER_CLI_SHA256={worker}");
    for spec in ["HEAD", "logs/HEAD"] {
        let output = std::process::Command::new("git")
            .args(["rev-parse", "--git-path", spec])
            .output()
            .expect("git build provenance");
        assert!(output.status.success());
        println!(
            "cargo:rerun-if-changed={}",
            String::from_utf8_lossy(&output.stdout).trim()
        );
    }
}
