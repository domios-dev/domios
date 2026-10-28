use super::*;

#[test]
fn crashes_and_unknown_submissions_never_rewrite_a_gateway_turn() {
    let mut previous = Admission {
        message_id: "message".into(),
        tab: "gateway".into(),
        state: AdmissionState::Claimed,
    };
    assert_eq!(resume(None, "message", "gateway"), Resume::Admit);
    assert_eq!(
        resume(Some(&previous), "message", "gateway"),
        Resume::Unconfirmed
    );
    previous.state = AdmissionState::SubmitUnconfirmed;
    assert_eq!(
        resume(Some(&previous), "message", "gateway"),
        Resume::Unconfirmed
    );
    previous.state = AdmissionState::Pending {
        delivery_id: "receipt".into(),
    };
    assert_eq!(
        resume(Some(&previous), "message", "gateway"),
        Resume::Wait("receipt".into())
    );
    previous.state = AdmissionState::Delivered;
    assert_eq!(
        resume(Some(&previous), "message", "gateway"),
        Resume::Delivered
    );
    assert_eq!(
        resume(Some(&previous), "message", "replacement"),
        Resume::WrongGateway
    );
    previous.state = AdmissionState::PrewriteRefused;
    assert_eq!(resume(Some(&previous), "message", "gateway"), Resume::Admit);
    assert_eq!(resume(Some(&previous), "next", "gateway"), Resume::Admit);
}

struct Requests {
    local: std::sync::atomic::AtomicUsize,
    refusals: std::sync::atomic::AtomicUsize,
}
#[async_trait]
impl RequestPorts for Requests {
    async fn local_turn(&self) -> Result<GatewayReceipt> {
        self.local
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(GatewayReceipt::Delivered)
    }
    async fn refusal(&self) -> Result<GatewayReceipt> {
        self.refusals
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(GatewayReceipt::Delivered)
    }
}
#[tokio::test]
async fn denied_requests_refuse_without_local_information_access_and_answers_do_not_loop() {
    use std::sync::atomic::Ordering;
    let ports = Requests {
        local: 0.into(),
        refusals: 0.into(),
    };
    for kind in [Kind::Question, Kind::TabRequest] {
        assert!(matches!(
            route_request(&Permission::default(), kind, false, &ports).await,
            Ok(GatewayReceipt::Delivered)
        ));
    }
    assert_eq!(ports.local.load(Ordering::Relaxed), 0);
    assert_eq!(ports.refusals.load(Ordering::Relaxed), 2);
    route_request(&Permission::default(), Kind::Answer, false, &ports)
        .await
        .unwrap();
    route_request(&Permission::default(), Kind::Refusal, false, &ports)
        .await
        .unwrap();
    assert_eq!(ports.refusals.load(Ordering::Relaxed), 2);
    let permission = Permission {
        allowed: true,
        scope: "opaque scope".into(),
        allow_tab_requests: false,
        allow_interactive_content: false,
        ..Default::default()
    };
    route_request(&permission, Kind::Question, false, &ports)
        .await
        .unwrap();
    route_request(&permission, Kind::TabRequest, false, &ports)
        .await
        .unwrap();
    assert_eq!(ports.local.load(Ordering::Relaxed), 1);
    assert_eq!(ports.refusals.load(Ordering::Relaxed), 3);
}

#[tokio::test]
async fn paused_requests_refuse_without_invoking_the_concierge_and_resume_restores_admission() {
    use std::sync::atomic::Ordering;
    let ports = Requests {
        local: 0.into(),
        refusals: 0.into(),
    };
    let permission = Permission {
        allowed: true,
        scope: "opaque".into(),
        allow_tab_requests: true,
        ..Default::default()
    };
    for kind in [
        Kind::Question,
        Kind::TabRequest,
        Kind::Answer,
        Kind::Refusal,
    ] {
        route_request(&permission, kind, true, &ports)
            .await
            .unwrap();
    }
    assert_eq!(ports.local.load(Ordering::Relaxed), 0);
    assert_eq!(ports.refusals.load(Ordering::Relaxed), 2);
    route_request(&permission, Kind::Question, false, &ports)
        .await
        .unwrap();
    assert_eq!(ports.local.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn permitted_answers_and_refusals_never_create_concierge_turns_or_reply_loops() {
    use std::sync::atomic::Ordering;
    let ports = Requests {
        local: 0.into(),
        refusals: 0.into(),
    };
    let permission = Permission {
        allowed: true,
        scope: "opaque".into(),
        allow_tab_requests: true,
        ..Default::default()
    };
    for paused in [false, true] {
        for kind in [Kind::Answer, Kind::Refusal] {
            route_request(&permission, kind, paused, &ports)
                .await
                .unwrap();
        }
    }
    assert_eq!(ports.local.load(Ordering::Relaxed), 0);
    assert_eq!(ports.refusals.load(Ordering::Relaxed), 0);
}
