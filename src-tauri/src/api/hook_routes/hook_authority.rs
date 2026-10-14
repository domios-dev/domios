use axum::Json;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HookAuthority {
    Telemetry,
    BindingAndStatus,
}

pub(super) fn authority(structured_backing: bool) -> HookAuthority {
    if structured_backing {
        HookAuthority::Telemetry
    } else {
        HookAuthority::BindingAndStatus
    }
}

pub(super) fn telemetry_response(tab: &str, event: &str) -> Option<Json<serde_json::Value>> {
    let structured = crate::service::codex_rpc::lifecycle::for_tab(tab).is_some();
    if authority(structured) != HookAuthority::Telemetry {
        return None;
    }
    log::debug!("[hook-telemetry] tab={tab} event={event} authority=app-server");
    Some(Json(
        serde_json::json!({"ok":true,"telemetry_only":true,"authority":"app-server"}),
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
        let apply_hook = |structured, session: &mut &str, busy: &mut bool| {
            if authority(structured) == HookAuthority::BindingAndStatus {
                *session = "other-hook-root";
                *busy = false;
            }
        };
        apply_hook(true, &mut session, &mut busy);
        assert_eq!(session, "server-root");
        assert!(busy);
        apply_hook(false, &mut session, &mut busy);
        assert_eq!(session, "other-hook-root");
        assert!(!busy);
    }
}
const _SPLIT_METRIC_PROBE: u8 = 0;
