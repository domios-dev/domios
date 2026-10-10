use super::ServiceContext;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Binding {
    pub session_id: String,
    pub nonce: String,
    pub base_url: String,
    pub cwd: String,
}

fn acknowledged<'a>(
    session: Option<&'a str>,
    nonce: Option<&'a str>,
    reported: Option<&str>,
    restored: bool,
) -> Option<(&'a str, &'a str)> {
    let nonce = nonce?;
    (!restored && reported == Some(nonce)).then_some((session?, nonce))
}

pub(super) fn current(svc: &ServiceContext, tab_id: &str) -> Option<Binding> {
    let (session_id, nonce) = {
        let mux = svc.mux.read();
        let tab = mux.get_tab(tab_id)?;
        let (session, nonce) = acknowledged(
            tab.agent_session_id.as_deref(),
            tab.session_binding_nonce.as_deref(),
            tab.reported_session_binding_nonce.as_deref(),
            tab.restored_from_snapshot,
        )?;
        (session.to_string(), nonce.to_string())
    };
    let (base_url, cwd) = super::super::structured_endpoint::sse_for_nonce(tab_id, &nonce)?;
    Some(Binding {
        session_id,
        nonce,
        base_url,
        cwd,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restored_session_waits_for_current_launch_acknowledgement() {
        assert_eq!(
            acknowledged(Some("session"), Some("next"), Some("previous"), false),
            None
        );
        assert_eq!(
            acknowledged(Some("session"), Some("next"), None, false),
            None
        );
        assert_eq!(
            acknowledged(Some("session"), Some("previous"), Some("previous"), true),
            None
        );
        assert_eq!(
            acknowledged(Some("session"), Some("next"), Some("next"), false),
            Some(("session", "next"))
        );
    }

    #[test]
    fn subscriber_identity_changes_for_nonce_session_or_endpoint() {
        let binding = Binding {
            session_id: "session".into(),
            nonce: "previous".into(),
            base_url: "http://127.0.0.1:1".into(),
            cwd: "/work".into(),
        };
        let mut next = binding.clone();
        next.nonce = "next".into();
        assert_ne!(binding, next);
        next = binding.clone();
        next.session_id = "other".into();
        assert_ne!(binding, next);
        next = binding.clone();
        next.base_url = "http://127.0.0.1:2".into();
        assert_ne!(binding, next);
    }

    #[test]
    fn previous_endpoint_cannot_be_acquired_with_new_launch_nonce() {
        let tab = "endpoint-launch-binding-regression";
        super::super::register_endpoint(tab, "http://127.0.0.1:1".into(), "/work".into());
        use super::super::super::structured_endpoint;
        structured_endpoint::bind_sse_launch(tab, "previous");
        assert!(structured_endpoint::sse_for_nonce(tab, "next").is_none());
        super::super::register_endpoint(tab, "http://127.0.0.1:2".into(), "/work".into());
        assert!(structured_endpoint::sse_for_nonce(tab, "previous").is_none());
        assert!(structured_endpoint::sse_for_nonce(tab, "next").is_none());
        structured_endpoint::bind_sse_launch(tab, "next");
        assert_eq!(
            structured_endpoint::sse_for_nonce(tab, "next"),
            Some(("http://127.0.0.1:2".into(), "/work".into()))
        );
        super::super::clear_tab(tab);
    }
}
