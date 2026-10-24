use super::*;
use crate::agent::provider::{AgentProvider, StarterPrompt, SystemPromptDelivery};
use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
use std::sync::Arc;

#[test]
fn general_bootstrap_composition_follows_each_provider_capability() {
    let plugins = crate::test_support::fixture_manifest_dir().join("../../../plugins");
    let mut providers = 0;
    let mut runtimes = 0;
    for entry in std::fs::read_dir(plugins).unwrap() {
        let directory = entry.unwrap().path();
        let path = directory.join("plugin.json");
        if !path.is_file() {
            continue;
        }
        let mut manifest: PluginManifest =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        if !manifest.general_agent_offer_when_absent {
            continue;
        }
        manifest.id = format!("opaque-provider-{providers}");
        let source = std::fs::read_to_string(directory.join("plugin.js")).unwrap();
        let vm = PluginVm::from_source(&manifest.id, source).unwrap();
        let provider = JsProvider::new(manifest, Arc::new(vm));
        providers += 1;
        runtimes += usize::from(provider.agent_runtime().is_some());
        let delivery = provider.system_prompt_delivery();
        assert!(matches!(
            delivery,
            SystemPromptDelivery::CliFlag(_) | SystemPromptDelivery::SessionConfig
        ));
        for bootstrap in [None, Some("fragment-b72\nλ $() `opaque`\nfragment-c38")] {
            let expected = match bootstrap {
                Some(bootstrap) => format!("fragment-a19\n\n{bootstrap}"),
                None => "fragment-a19".into(),
            };
            let pty = compose_launch_instructions(
                "fragment-a19".into(),
                bootstrap.map(str::to_owned),
                &delivery,
            );
            assert_eq!(pty.task.as_deref(), Some(expected.as_str()));
            assert!(pty.startup_message.is_none());
            assert_eq!(
                pty.starter_prompt,
                if bootstrap.is_some() {
                    StarterPrompt::AssignedTask
                } else {
                    StarterPrompt::NoStarter
                }
            );
            if provider.agent_runtime().is_some() {
                let config = runtime_config("fragment-a19".into(), None, None, false);
                assert_eq!(config["systemPrompt"], "fragment-a19");
            }
        }
    }
    assert_eq!(providers, 3);
    assert_eq!(runtimes, 1);
}

#[test]
fn general_runtime_config_preserves_permanent_context_and_tab_controls() {
    for bypass in [false, true] {
        let config = runtime_config(
            "opaque-general-developer-context".into(),
            Some("opaque-model".into()),
            Some("opaque-effort".into()),
            bypass,
        );
        assert_eq!(config["systemPrompt"], "opaque-general-developer-context");
        assert_eq!(
            crate::service::agent_runtime::model::settings_from_config(&config),
            codeterm_plugin::agent_runtime::RuntimeSettings {
                model: Some("opaque-model".into()),
                effort: Some("opaque-effort".into()),
                mode: Some(
                    if bypass {
                        "full-access"
                    } else {
                        "auto-accept-edits"
                    }
                    .into()
                ),
                ..Default::default()
            }
        );
        assert!(config.get("task").is_none());
        assert!(config.get("env").is_none());
    }
}

#[path = "tests/capture.rs"]
mod capture;

#[test]
fn general_runtime_restore_keeps_the_exact_thread_and_fresh_sibling_stays_independent() {
    assert_eq!(
        general_agent_session_plan(true, false, Some("opaque-own-thread".into())),
        GeneralAgentSessionPlan::Restore("opaque-own-thread".into())
    );
    assert_eq!(
        LaunchOwnership::Main.select_session(Some("opaque-own-thread".into())),
        Some("opaque-own-thread".into())
    );
    assert!(LaunchOwnership::FreshSession
        .select_session(Some("opaque-own-thread".into()))
        .is_none());
    assert!(!LaunchOwnership::FreshSession.bootstrap());
}
