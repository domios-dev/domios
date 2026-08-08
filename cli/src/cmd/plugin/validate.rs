//! `codeterm plugin validate <dir>` — author-facing preflight for a plugin
//! bundle. Runtime truth is reused, never re-interpreted: the strict daemon
//! `PluginManifest` (deny_unknown_fields) and the QuickJS loader (`load_one`).

use std::path::Path;

use codeterm_plugin::js_runtime::{has_method, load_one, JsProvider, PluginManifest};

/// AgentProvider exports the JS bridge probes, paired with what serves the
/// method when the export is absent (the JsProvider fallback chain).
const AGENT_PROVIDER_EXPORTS: &[(&str, &str)] = &[
    (
        "buildLaunchCommand",
        "NONE — launch command resolves empty; only a bundled drop-in falls back to its displaced native provider",
    ),
    ("buildResumeCommand", "none — session resume unavailable"),
    ("buildResumeCommandWithContext", "plain buildResumeCommand + args"),
    ("starterPromptText", "host-owned starter-prompt text"),
    ("detectFromTitle", "always false (no title detection)"),
    ("detectFromOutput", "always false (no output detection)"),
    ("screenHasTui", "always false"),
    ("isOutputNoise", "always false (nothing filtered)"),
    ("hasEventMarkers", "generic marker heuristic"),
    ("detectEvents", "universal prompt detector"),
    ("extractSessionName", "none"),
    ("parsePrompt", "universal permission-prompt parser"),
    ("buildPromptResponse", "arrow-down + enter"),
    ("directoryTrustPromptResponse", "none"),
    ("sessionFilePath", "manifest sessions.* layout"),
    ("sessionExists", "sessionFilePath existence check"),
    ("sessionCreatedMs", "none"),
    ("detectSessionId", "manifest sessions.* uuid-recency heuristic"),
    ("detectActiveSession", "none"),
    ("detectSessionModel", "none"),
    ("detectSessionReasoningEffort", "none"),
    ("enumerateSessions", "manifest sessions.* directory scan"),
    ("describeSession", "none (sessions list without title/summary)"),
    ("parseSessionMessages", "native fallback (bundled drop-ins only), else empty"),
    ("parseSessionDelta", "none — chat tailing yields no messages"),
    ("sessionJsonlPath", "native fallback (bundled drop-ins only); chat UI stays off without it"),
    ("usesStructuredChat", "native fallback (bundled drop-ins only), else false"),
    ("readStructuredChat", "native fallback (bundled drop-ins only), else empty"),
    ("parseInteractiveEvents", "native fallback (bundled drop-ins only); also requires spawn.interactiveEvents"),
    ("classifyTabState", "native fallback (bundled drop-ins only), else no classification"),
    ("providerSelfUpdateCapability", "native fallback (bundled drop-ins only), else none"),
    ("settingsSchema", "manifest-derived settings sections"),
    ("notificationsSchema", "none"),
    ("parseUsage", "usage parsing off"),
    ("fetchUsage", "usage fetch off"),
    ("discoverModels", "manifest models list"),
    ("checkIntegration", "no integration status"),
    ("installIntegration", "none"),
    ("uninstallIntegration", "none"),
    ("restoreIntegration", "none"),
    ("installBridgeRule", "manifest rule.* file install"),
    ("uninstallBridgeRule", "manifest rule.* file removal"),
];

#[derive(Debug)]
pub struct ValidationReport {
    pub id: String,
    pub version: String,
    pub capabilities: Vec<&'static str>,
    /// (export name, implemented in plugin.js, fallback description).
    pub exports: Vec<(&'static str, bool, &'static str)>,
    pub warnings: Vec<String>,
}

pub fn run(json: bool, dir: &Path) {
    match validate_dir(dir) {
        Ok(report) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&report.to_json()).unwrap());
            } else {
                print!("{}", report.render(dir));
            }
        }
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}

pub fn validate_dir(dir: &Path) -> Result<ValidationReport, String> {
    let manifest_path = dir.join("plugin.json");
    if !manifest_path.is_file() {
        return Err(format!("no plugin.json in {}", dir.display()));
    }
    // Strict parse first so a manifest typo fails naming the key, before any VM work.
    let manifest =
        PluginManifest::load(&manifest_path).map_err(|e| format!("manifest: {e}"))?;
    // The daemon's real load path: entry existence, hostApi conformance, QuickJS eval.
    let provider = load_one(&manifest_path).map_err(|e| format!("load: {e}"))?;
    Ok(build_report(&manifest, &provider))
}

fn build_report(manifest: &PluginManifest, provider: &JsProvider) -> ValidationReport {
    let mut exports = Vec::new();
    let mut warnings = Vec::new();
    if manifest.capabilities.agent_provider {
        for (name, fallback) in AGENT_PROVIDER_EXPORTS {
            exports.push((*name, has_method(provider.vm(), name), *fallback));
        }
        if !has_method(provider.vm(), "buildLaunchCommand") {
            warnings.push(
                "buildLaunchCommand is not exported — the launch command resolves empty, so a \
                 third-party provider cannot spawn (only bundled drop-ins fall back to a \
                 displaced native provider)"
                    .to_string(),
            );
        }
    }
    ValidationReport {
        id: manifest.id.clone(),
        version: manifest.version.clone(),
        capabilities: enabled_capabilities(manifest),
        exports,
        warnings,
    }
}

fn enabled_capabilities(m: &PluginManifest) -> Vec<&'static str> {
    let c = &m.capabilities;
    let mut out = Vec::new();
    if c.agent_provider {
        out.push("agentProvider");
    }
    if c.secret_backend {
        out.push("secretBackend");
    }
    if c.transcriber {
        out.push("transcriber");
    }
    if c.glance_view {
        out.push("glanceView");
    }
    if c.status_bubble {
        out.push("statusBubble");
    }
    if c.view {
        out.push("view");
    }
    if c.chat_backend.enabled() {
        out.push("chatBackend");
    }
    if c.chat_preprocessor.enabled() {
        out.push("chatPreprocessor");
    }
    out
}

impl ValidationReport {
    fn render(&self, dir: &Path) -> String {
        let mut out = String::new();
        out.push_str(&format!("plugin: {} v{} ({})\n", self.id, self.version, dir.display()));
        out.push_str("manifest: OK (strict daemon parse)\n");
        out.push_str("load: OK (QuickJS eval)\n");
        out.push_str(&format!("capabilities: {}\n", self.capabilities.join(", ")));
        if !self.exports.is_empty() {
            out.push_str("agentProvider exports:\n");
            for (name, found, fallback) in &self.exports {
                if *found {
                    out.push_str(&format!("  js        {name}\n"));
                } else {
                    out.push_str(&format!("  fallback  {name} — {fallback}\n"));
                }
            }
        }
        if !self.warnings.is_empty() {
            out.push_str("warnings:\n");
            for w in &self.warnings {
                out.push_str(&format!("  {w}\n"));
            }
        }
        out
    }

    fn to_json(&self) -> serde_json::Value {
        let js: Vec<&str> = self
            .exports
            .iter()
            .filter(|(_, found, _)| *found)
            .map(|(n, _, _)| *n)
            .collect();
        let fallback: serde_json::Map<String, serde_json::Value> = self
            .exports
            .iter()
            .filter(|(_, found, _)| !*found)
            .map(|(n, _, f)| (n.to_string(), serde_json::Value::String(f.to_string())))
            .collect();
        serde_json::json!({
            "id": self.id,
            "version": self.version,
            "manifest": "ok",
            "load": "ok",
            "capabilities": self.capabilities,
            "exports": { "js": js, "fallback": fallback },
            "warnings": self.warnings,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    /// Matches what a real third-party author writes: hostApi literal 2
    /// (`host_api::HOST_API_VERSION` is crate-private by design).
    fn valid_manifest(id: &str) -> String {
        format!(
            r#"{{
                "id": "{id}",
                "version": "0.1.0",
                "displayName": "{id} Test",
                "hostApi": 2,
                "spawn": {{
                    "readinessStrategy": {{ "kind": "fixed_delay", "durationMs": 0 }},
                    "taskDelivery": "pty_input",
                    "verifySubmit": {{ "kind": "none" }}
                }},
                "entry": "plugin.js"
            }}"#
        )
    }

    fn thin_plugin_js(id: &str) -> String {
        format!(
            r#"module.exports.default = {{
                buildLaunchCommand() {{ return "{id}"; }},
                detectFromTitle(t) {{ return t.includes("{id}"); }}
            }};"#
        )
    }

    #[test]
    fn plugin_validate_typoed_spawn_key_names_the_key() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("typo");
        // "taskDelivry" — the typo the strict parser must call out by name.
        write(
            &dir.join("plugin.json"),
            r#"{
                "id": "typo",
                "version": "0.1.0",
                "displayName": "Typo Test",
                "hostApi": 2,
                "spawn": {
                    "readinessStrategy": { "kind": "fixed_delay", "durationMs": 0 },
                    "taskDelivry": "pty_input",
                    "verifySubmit": { "kind": "none" }
                },
                "entry": "plugin.js"
            }"#,
        );
        write(&dir.join("plugin.js"), "module.exports.default = {};");
        let err = validate_dir(&dir).expect_err("typo'd spawn key must fail");
        assert!(err.starts_with("manifest:"), "fails in the manifest phase: {err}");
        assert!(err.contains("taskDelivry"), "error names the typo'd key: {err}");
    }

    #[test]
    fn plugin_validate_throwing_entry_surfaces_js_error() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("thrower");
        write(&dir.join("plugin.json"), &valid_manifest("thrower"));
        write(
            &dir.join("plugin.js"),
            r#"throw new Error("boom-at-eval");"#,
        );
        let err = validate_dir(&dir).expect_err("throwing plugin.js must fail");
        assert!(err.starts_with("load:"), "fails in the load phase: {err}");
        assert!(err.contains("boom-at-eval"), "error carries the JS message: {err}");
    }

    #[test]
    fn plugin_validate_thin_plugin_passes_listing_capabilities() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("thin");
        write(&dir.join("plugin.json"), &valid_manifest("thin"));
        write(&dir.join("plugin.js"), &thin_plugin_js("thin"));
        let report = validate_dir(&dir).expect("valid thin plugin passes");
        assert_eq!(report.id, "thin");
        assert_eq!(report.capabilities, vec!["agentProvider"]);
        let found: Vec<&str> = report
            .exports
            .iter()
            .filter(|(_, f, _)| *f)
            .map(|(n, _, _)| *n)
            .collect();
        assert_eq!(found, vec!["buildLaunchCommand", "detectFromTitle"]);
        let (_, classify_found, _) = report
            .exports
            .iter()
            .find(|(n, _, _)| *n == "classifyTabState")
            .expect("classifyTabState probed");
        assert!(!classify_found, "unexported method reports as fallback");
        assert!(report.warnings.is_empty(), "launch export present — no warning");
    }

    #[test]
    fn plugin_validate_missing_launch_export_warns_but_passes() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("nolaunch");
        write(&dir.join("plugin.json"), &valid_manifest("nolaunch"));
        write(&dir.join("plugin.js"), "module.exports.default = {};");
        let report = validate_dir(&dir).expect("loadable plugin passes even without launch");
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("buildLaunchCommand"));
    }

    #[test]
    fn plugin_validate_missing_manifest_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("empty");
        std::fs::create_dir_all(&dir).unwrap();
        let err = validate_dir(&dir).expect_err("no plugin.json must fail");
        assert!(err.contains("no plugin.json"), "got: {err}");
    }

    #[test]
    fn plugin_validate_wrong_host_api_fails_at_load() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("oldapi");
        write(
            &dir.join("plugin.json"),
            &valid_manifest("oldapi").replace(r#""hostApi": 2"#, r#""hostApi": 999"#),
        );
        write(&dir.join("plugin.js"), "module.exports.default = {};");
        let err = validate_dir(&dir).expect_err("wrong hostApi must fail");
        assert!(err.starts_with("load:"), "conformance gate is a load failure: {err}");
        assert!(err.contains("hostApi"), "error names the gate: {err}");
    }
}
