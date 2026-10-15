#[path = "build/plugin_bundles.rs"]
mod plugin_bundles;

fn main() {
    embed_build_commit();
    plugin_bundles::generate_system_plugin_hashes();

    // Ensure all Windows targets (including test binaries) get the Common Controls v6 manifest.
    // tauri-winres misses test runners, so linker embedding uses /MANIFEST:EMBED + /MANIFESTINPUT.
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=windows-app-manifest.xml");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        let manifest =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("windows-app-manifest.xml");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }

    #[cfg(windows)]
    {
        let attrs = tauri_build::Attributes::new()
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
        tauri_build::try_build(attrs).expect("failed to run tauri-build");
    }
    #[cfg(target_os = "macos")]
    {
        embed_macos_info_plist();
    }
    #[cfg(not(windows))]
    {
        tauri_build::build();
    }
}

/// Only the root package tracks HEAD, so a new commit relinks the app instead of rebuilding every crate.
fn embed_build_commit() {
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
    };
    let full_hash = git(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    let short_hash =
        git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=BUILD_SHA={}", full_hash.trim());
    println!("cargo:rustc-env=BUILD_HASH={}", short_hash.trim());

    // --git-path is worktree-safe; logs/HEAD changes on every commit/checkout.
    for spec in ["HEAD", "logs/HEAD"] {
        if let Some(path) = git(&["rev-parse", "--git-path", spec]) {
            println!("cargo:rerun-if-changed={}", path.trim());
        }
    }
}

#[cfg(target_os = "macos")]
fn embed_macos_info_plist() {
    let profile = std::env::var("PROFILE").unwrap_or_default();
    let is_debug = profile == "debug";
    let identifier = if is_debug {
        "com.codeterm.dev"
    } else {
        "com.codeterm.app"
    };
    let name = if is_debug { "CodeTerm Dev" } else { "CodeTerm" };
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR not set"));
    let plist = out_dir.join("embedded-Info.plist");
    let body = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>codeterm</string>
  <key>CFBundleIdentifier</key>
  <string>{identifier}</string>
  <key>CFBundleName</key>
  <string>{name}</string>
  <key>NSMicrophoneUsageDescription</key>
  <string>CodeTerm records voice notes so they can be attached to chat messages.</string>
</dict>
</plist>
"#,
    );
    std::fs::write(&plist, body).expect("write embedded macOS Info.plist");
    println!("cargo:rerun-if-changed=Info.plist");
    println!(
        "cargo:rustc-link-arg=-Wl,-sectcreate,__TEXT,__info_plist,{}",
        plist.display()
    );
}
