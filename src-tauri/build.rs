fn main() {
    // Inject git commit hash as BUILD_HASH env var at compile time
    let hash = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=BUILD_HASH={}", hash.trim());

    // Inject BUILD_NUMBER from file (incremented by build.sh dev)
    let build_num = std::fs::read_to_string("../BUILD_NUMBER")
        .unwrap_or_else(|_| "0".to_string());
    println!("cargo:rustc-env=BUILD_NUMBER={}", build_num.trim());

    tauri_build::build();
}
