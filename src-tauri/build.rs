fn main() {
    let hash = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=BUILD_HASH={}", hash.trim());

    // Embed the Common Controls v6 manifest into EVERY target on
    // Windows (bins + test runners). tauri-build only embeds it into
    // the main bin via tauri-winres, which leaves `cargo test --lib`
    // exes without it — and without the manifest, comctl32.dll v6
    // entry points (TaskDialogIndirect, etc) aren't resolved at load
    // time and the test binary fails with STATUS_ENTRYPOINT_NOT_FOUND
    // (0xc0000139). See tauri-apps/tauri#14580.
    //
    // Two pieces:
    //  1. Tell tauri-build NOT to embed its own app manifest (would
    //     conflict with mt.exe on merge → c1010070 parse error).
    //  2. Tell linker to embed ours via /MANIFEST:EMBED + /MANIFESTINPUT.
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=windows-app-manifest.xml");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("windows-app-manifest.xml");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }

    #[cfg(windows)]
    {
        let attrs = tauri_build::Attributes::new().windows_attributes(
            tauri_build::WindowsAttributes::new_without_app_manifest(),
        );
        tauri_build::try_build(attrs).expect("failed to run tauri-build");
    }
    #[cfg(not(windows))]
    {
        tauri_build::build();
    }
}
