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
