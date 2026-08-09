//! Manifest launch/resume template contract: a declared template serves a
//! manifest-only drop-in, the JS export always wins over it, and absence of
//! both reproduces today's behavior. Expected strings round-trip from the
//! fixture constants below — never from baked prose.

use super::*;
use crate::provider::AgentResumeParams;

const LAUNCH_TEMPLATE: &str =
    "fixture-bin {skipPermissions} {args} --session-id {sessionId} --task-file {task}";
const RESUME_TEMPLATE: &str = "fixture-bin {skipPermissions} --continue {sessionId}";
const AUTO_APPROVE_FLAG: &str = "--fixture-yolo";
const MODEL_FLAG: &str = "--fixture-model";
const EFFORT_FLAG: &str = "--fixture-effort";

fn template_manifest_json(id: &str, launch: &str, resume: &str) -> String {
    format!(
        r#"{{"id":"{id}","version":"0.1.0","displayName":"Fixture",
            "commands":{{
                "launchArgsFormat":"{launch}",
                "resumeArgsFormat":"{resume}",
                "autoApproveFlags":["{AUTO_APPROVE_FLAG}"],
                "modelLaunchArgsFormat":["{MODEL_FLAG}","{{model}}"],
                "reasoningEffortLaunchArgsFormat":["{EFFORT_FLAG}","{{effort}}"]
            }},
            "spawn":{{"readinessStrategy":{{"kind":"fixed_delay","durationMs":0}},
                     "taskDelivery":"pty_input","verifySubmit":{{"kind":"none"}}}}}}"#
    )
}

fn load_template_fixture(id: &str, launch: &str, resume: &str, plugin_js: &str) -> JsProvider {
    use std::sync::Arc;

    let manifest_json = template_manifest_json(id, launch, resume);
    let manifest: crate::js_runtime::PluginManifest = serde_json::from_str(&manifest_json)
        .expect("template fixture manifest parses through the strict daemon shape");
    let vm = crate::js_runtime::PluginVm::from_source_with(
        id,
        plugin_js.to_string(),
        Some(manifest_json),
    )
    .expect("fixture vm");
    JsProvider::new(manifest, Arc::new(vm))
}

/// Matrix case 1: no JS launch/resume export ⇒ the manifest template serves
/// both, with task, model+effort (host-composed into args), skip-permissions,
/// extra args, and sessionId all substituting from the same fixture values.
#[test]
fn manifest_launch_template_fixture_matrix_round_trips() {
    let p = load_template_fixture(
        "manifest-template-matrix",
        LAUNCH_TEMPLATE,
        RESUME_TEMPLATE,
        "module.exports.default = {};",
    );

    // Model/effort flags reach launch through the host-composed args, exactly
    // like the existing modelLaunchArgsFormat mechanism — round-trip them.
    let mut args = p.model_launch_args("model-x");
    args.extend(p.reasoning_effort_launch_args("effort-y"));
    assert_eq!(
        args,
        vec![
            MODEL_FLAG.to_string(),
            "model-x".to_string(),
            EFFORT_FLAG.to_string(),
            "effort-y".to_string(),
        ],
    );
    args.push("--extra".into());
    args.push("extra-value".into());

    let params = AgentLaunchParams {
        task: Some("task-payload".into()),
        args: Some(args.clone()),
        skip_permissions: true,
        session_id: Some("sid-fixture-1".into()),
        ..Default::default()
    };
    let expected = LAUNCH_TEMPLATE
        .replace("{skipPermissions}", AUTO_APPROVE_FLAG)
        .replace("{args}", &args.join(" "))
        .replace("{sessionId}", "sid-fixture-1")
        .replace("{task}", "task-payload");
    assert_eq!(p.build_launch_command(&params), expected);
    assert!(
        !expected.contains('{') && !expected.contains('}'),
        "no placeholder residue may survive substitution: {expected}"
    );

    // Every optional parameter absent + skip-permissions off ⇒ empty values.
    let bare = AgentLaunchParams::default();
    let expected_bare = LAUNCH_TEMPLATE
        .replace("{skipPermissions}", "")
        .replace("{args}", "")
        .replace("{sessionId}", "")
        .replace("{task}", "");
    assert_eq!(p.build_launch_command(&bare), expected_bare);

    let expected_resume = RESUME_TEMPLATE
        .replace("{skipPermissions}", AUTO_APPROVE_FLAG)
        .replace("{sessionId}", "sid-fixture-2");
    assert_eq!(
        p.build_resume_command("sid-fixture-2", true).as_deref(),
        Some(expected_resume.as_str())
    );
    let expected_resume_bare = RESUME_TEMPLATE
        .replace("{skipPermissions}", "")
        .replace("{sessionId}", "sid-fixture-2");
    assert_eq!(
        p.build_resume_command("sid-fixture-2", false).as_deref(),
        Some(expected_resume_bare.as_str())
    );

    // Resume-with-context splices extra args through the existing append path.
    let resumed = p.build_resume_command_with_context(&AgentResumeParams {
        session_id: "sid-fixture-2".into(),
        skip_permissions: true,
        system_prompt: None,
        args: vec!["--extra".into()],
    });
    assert_eq!(
        resumed,
        Some(format!("{expected_resume} --extra")),
        "template resume must feed the context path via append_launch_args"
    );
}

/// Matrix case 2 (negative control): a plugin WITH the JS exports is untouched
/// by the declared templates — the export wins for both launch and resume.
#[test]
fn manifest_launch_template_js_export_wins() {
    let p = load_template_fixture(
        "manifest-template-export-wins",
        LAUNCH_TEMPLATE,
        RESUME_TEMPLATE,
        r#"module.exports.default = {
            buildLaunchCommand(params) { return "js-launch " + (params.task || "none"); },
            buildResumeCommand(sessionId, skipPermissions) {
                return "js-resume " + sessionId + " " + skipPermissions;
            }
        };"#,
    );
    let params = AgentLaunchParams {
        task: Some("task-payload".into()),
        skip_permissions: true,
        ..Default::default()
    };
    let launch = p.build_launch_command(&params);
    assert_eq!(launch, "js-launch task-payload");
    assert!(
        !launch.contains("fixture-bin"),
        "template must not leak into an exported launch: {launch}"
    );
    assert_eq!(
        p.build_resume_command("sid-1", true).as_deref(),
        Some("js-resume sid-1 true")
    );
}

/// Matrix case 3 (absent-both control): neither export nor template ⇒ exactly
/// today's behavior — empty launch command and no resume command.
#[test]
fn manifest_launch_template_absent_both_keeps_todays_defaults() {
    let p = load_inline_js(
        "manifest-template-absent-both",
        "module.exports.default = {};",
    );
    assert!(p.manifest().commands.launch_args_format.is_none());
    assert!(p.manifest().commands.resume_args_format.is_none());
    assert_eq!(p.build_launch_command(&AgentLaunchParams::default()), "");
    assert_eq!(p.build_resume_command("sid-1", true), None);
}

/// A placeholder outside the closed set never half-renders: the template is
/// ignored at runtime (today's defaults) instead of emitting braces, and the
/// validate verb is the loud gate that names the offender.
#[test]
fn manifest_launch_template_unknown_placeholder_never_emits_braces() {
    let p = load_template_fixture(
        "manifest-template-unknown-placeholder",
        "fixture-bin {unknwn}",
        "fixture-bin {sessoinId}",
        "module.exports.default = {};",
    );
    assert_eq!(p.build_launch_command(&AgentLaunchParams::default()), "");
    assert_eq!(p.build_resume_command("sid-1", true), None);
}
