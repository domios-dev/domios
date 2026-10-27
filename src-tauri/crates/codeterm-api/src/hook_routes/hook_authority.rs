use axum::Json;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HookAuthority {
    Telemetry,
    BindingAndStatus,
}

pub(super) fn authority(runtime_capability: bool, runtime_backing: bool) -> HookAuthority {
    if runtime_capability && runtime_backing {
        HookAuthority::Telemetry
    } else {
        HookAuthority::BindingAndStatus
    }
}

pub(super) fn telemetry_response(
    svc: &codeterm_service_core::service::ServiceContext,
    tab: &str,
    event: &str,
) -> Option<Json<serde_json::Value>> {
    let structured = codeterm_service_core::service::agent_runtime::requested_for_tab(svc, tab);
    let runtime_backing = svc.mux.read().get_tab(tab).is_some_and(|tab| {
        matches!(
            tab.backing,
            codeterm_service_core::multiplexer::types::TabBacking::ChatBackend { .. }
        )
    });
    if authority(structured, runtime_backing) != HookAuthority::Telemetry {
        return None;
    }
    log::debug!("[hook-telemetry] tab={tab} event={event} authority=agent-runtime");
    Some(Json(
        serde_json::json!({"ok":true,"telemetry_only":true,"authority":"agent-runtime"}),
    ))
}

/// Verdict for a provider-reported Stop POST. A carried nonce must equal the
/// tab's own launch nonce (completion-via-report); a nonce-less POST stays on
/// the legacy inference-tolerant path; anything else is a foreign reporter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StopReportVerdict {
    Verified,
    Legacy,
    Foreign,
}

pub(super) fn stop_report_verdict(
    launch_nonce: Option<&str>,
    report_nonce: Option<&str>,
) -> StopReportVerdict {
    match (launch_nonce, report_nonce) {
        (Some(expected), Some(got)) if got == expected => StopReportVerdict::Verified,
        (_, Some(_)) => StopReportVerdict::Foreign,
        (_, None) => StopReportVerdict::Legacy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codex_rpc_hook_authority_preserves_structured_binding_and_pty_fallback() {
        let mut session = "server-root";
        let mut busy = true;
        let apply_hook = |capability, backing, session: &mut &str, busy: &mut bool| {
            if authority(capability, backing) == HookAuthority::BindingAndStatus {
                *session = "other-hook-root";
                *busy = false;
            }
        };
        apply_hook(true, true, &mut session, &mut busy);
        assert_eq!(session, "server-root");
        assert!(busy);
        for (capability, backing) in [(true, false), (false, true), (false, false)] {
            assert_eq!(
                authority(capability, backing),
                HookAuthority::BindingAndStatus
            );
        }
        apply_hook(true, false, &mut session, &mut busy);
        assert_eq!(session, "other-hook-root");
        assert!(!busy);
    }
}
