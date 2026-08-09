//! Manifest launch/resume template rendering — the declared-data fallback
//! used only when the corresponding JS export is absent, mirroring the
//! `modelLaunchArgsFormat` substitution shape.

use crate::js_runtime::manifest::{
    substitute_template, unknown_template_placeholder, PluginManifest,
    LAUNCH_TEMPLATE_PLACEHOLDERS, RESUME_TEMPLATE_PLACEHOLDERS,
};
use crate::provider::AgentLaunchParams;

/// `{skipPermissions}` expands to the manifest's declared auto-approve
/// flags when requested, and to nothing otherwise.
fn skip_permission_flags(manifest: &PluginManifest, skip_permissions: bool) -> String {
    skip_permissions
        .then(|| manifest.commands.auto_approve_flags.join(" "))
        .unwrap_or_default()
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
            ("sessionId", params.session_id.as_deref().unwrap_or_default()),
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
