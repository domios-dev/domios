#[path = "build/git_inputs.rs"]
mod git_inputs;
#[path = "../src-tauri/build/plugin_bundles.rs"]
mod plugin_bundles;

use std::process::Command;

fn main() {
    plugin_bundles::generate_system_plugin_hashes(std::path::Path::new(env!("CARGO_MANIFEST_DIR")));
    for path in ["HEAD", "packed-refs"] {
        if let Some(path) = git_output(&["rev-parse", "--git-path", path]) {
            if !std::path::Path::new(&path).exists() {
                continue;
            }
            println!("cargo:rerun-if-changed={path}");
        }
    }
    if let Some(reference) = git_output(&["symbolic-ref", "-q", "HEAD"]) {
        if let Some(path) = git_output(&["rev-parse", "--git-path", &reference]) {
            let input = git_inputs::existing_input(std::path::Path::new(&path));
            println!("cargo:rerun-if-changed={}", input.display());
        }
    }

    let sha = git_output(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=CODETERM_CLI_BUILD_SHA={sha}");
}

fn git_output(args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
