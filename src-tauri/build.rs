fn main() {
    // Inject git commit hash as BUILD_HASH env var at compile time
    let hash = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=BUILD_HASH={}", hash.trim());

    tauri_build::build();
}
