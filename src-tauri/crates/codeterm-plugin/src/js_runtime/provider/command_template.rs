//! Manifest launch/resume template rendering — the declared-data fallback
//! used only when the corresponding JS export is absent, mirroring the
//! `modelLaunchArgsFormat` substitution shape.

use crate::js_runtime::manifest::{
    substitute_template, unknown_template_placeholder, PluginManifest,
    LAUNCH_TEMPLATE_PLACEHOLDERS, RESUME_TEMPLATE_PLACEHOLDERS,
};
use crate::provider::AgentLaunchParams;

pub(super) fn launch_spec_from_template(
    manifest: &PluginManifest,
    params: &AgentLaunchParams,
) -> Option<codeterm_types::agent::LaunchSpec> {
    let mut spec = manifest.commands.launch_spec.clone()?;
    let render = |value: &str| -> Option<String> {
        checked_template(
            manifest,
            "launchSpec",
            Some(value),
            LAUNCH_TEMPLATE_PLACEHOLDERS,
        )?;
        if value.contains("{args}") || value.contains("{skipPermissions}") {
            return None;
        }
        Some(substitute_template(
            value,
            &[
                ("task", params.task.as_deref().unwrap_or_default()),
                (
                    "sessionId",
                    params.session_id.as_deref().unwrap_or_default(),
                ),
            ],
        ))
    };
    spec.program = render(&spec.program)?;
    let mut args = Vec::new();
    for arg in &spec.args {
        match arg.as_str() {
            "{args}" => args.extend(params.args.clone().unwrap_or_default()),
            "{skipPermissions}" if params.skip_permissions => {
                args.extend(manifest.commands.auto_approve_flags.clone());
            }
            "{skipPermissions}" => {}
            _ => args.push(render(arg)?),
        }
    }
    spec.args = args;
    spec.env_set = spec
        .env_set
        .iter()
        .map(|(key, value)| Some((key.clone(), render(value)?)))
        .collect::<Option<_>>()?;
    (!spec.program.trim().is_empty()).then_some(spec)
}

/// `{skipPermissions}` expands to the manifest's declared auto-approve
/// flags when requested, and to nothing otherwise.
fn skip_permission_flags(manifest: &PluginManifest, skip_permissions: bool) -> String {
    if skip_permissions {
        manifest.commands.auto_approve_flags.join(" ")
    } else {
        String::new()
    }
}

/// A placeholder outside the closed set never half-renders: warn and ignore
/// the template so behavior stays exactly the absent-template default.
fn checked_template<'m>(
    manifest: &'m PluginManifest,
    field: &str,
    template: Option<&'m str>,
    allowed: &[&str],
) -> Option<&'m str> {
    let template = template?;
    match unknown_template_placeholder(template, allowed) {
        None => Some(template),
        Some(name) => {
            log::warn!(
                "[plugin {}] {field}: unknown placeholder '{name}' — template ignored",
                manifest.id
            );
            None
        }
    }
}

pub(super) fn launch_command_from_template(
    manifest: &PluginManifest,
    params: &AgentLaunchParams,
) -> Option<String> {
    let template = checked_template(
        manifest,
        "launchArgsFormat",
        manifest.commands.launch_args_format.as_deref(),
        LAUNCH_TEMPLATE_PLACEHOLDERS,
    )?;
    let args = params.args.as_deref().unwrap_or_default().join(" ");
    let skip = skip_permission_flags(manifest, params.skip_permissions);
    Some(substitute_template(
        template,
        &[
            ("task", params.task.as_deref().unwrap_or_default()),
            ("args", args.as_str()),
            ("skipPermissions", skip.as_str()),
            (
                "sessionId",
                params.session_id.as_deref().unwrap_or_default(),
            ),
        ],
    ))
}

pub(super) fn resume_command_from_template(
    manifest: &PluginManifest,
    session_id: &str,
    skip_permissions: bool,
) -> Option<String> {
    let template = checked_template(
        manifest,
        "resumeArgsFormat",
        manifest.commands.resume_args_format.as_deref(),
        RESUME_TEMPLATE_PLACEHOLDERS,
    )?;
    let skip = skip_permission_flags(manifest, skip_permissions);
    Some(substitute_template(
        template,
        &[
            ("sessionId", session_id),
            ("skipPermissions", skip.as_str()),
        ],
    ))
}

pub(super) fn build_launch_command(
    provider: &super::JsProvider,
    params: &AgentLaunchParams,
) -> String {
    if !provider.has_js("buildLaunchCommand") {
        if let Some(command) = launch_command_from_template(&provider.manifest, params) {
            return command;
        }
    }
    provider
        .try_call::<String>(
            "buildLaunchCommand",
            &[super::wire::launch_params_to_json(params)],
        )
        .unwrap_or_default()
}

pub(super) fn launch_spec(
    provider: &super::JsProvider,
    params: &AgentLaunchParams,
) -> Option<codeterm_types::agent::LaunchSpec> {
    if provider.has_js("buildLaunchSpec") {
        return provider.try_call(
            "buildLaunchSpec",
            &[super::wire::launch_params_to_json(params)],
        );
    }
    launch_spec_from_template(&provider.manifest, params)
}
