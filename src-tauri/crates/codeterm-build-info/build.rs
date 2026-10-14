fn main() {
    let full_hash = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| "unknown".to_string());
    let short_hash = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=BUILD_SHA={}", full_hash.trim());
    println!("cargo:rustc-env=BUILD_HASH={}", short_hash.trim());

    // Re-embed BUILD_HASH when HEAD moves; --git-path is worktree-safe, logs/HEAD changes on every commit/checkout.
    for spec in ["HEAD", "logs/HEAD"] {
        if let Some(path) = std::process::Command::new("git")
            .args(["rev-parse", "--git-path", spec])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
        {
            println!("cargo:rerun-if-changed={}", path.trim());
        }
    }
}
