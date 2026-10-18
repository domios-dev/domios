use super::*;

/// The receiving hop's classification must still lead with its delivery code,
/// independent of the sender hop's transport classification.
#[test]
fn receiving_classification_still_leads_with_the_delivery_code() {
    use codeterm_mesh::contract::delivery;

    let submit_unconfirmed =
        codeterm_service_core::service::delivery_error::DeliveryError::SubmitUnconfirmed {
            tab: "53854ac8".into(),
            detail: "x".into(),
        }
        .to_json();
    assert!(relay_error_summary(502, &submit_unconfirmed).starts_with(delivery::SUBMIT_UNCONFIRMED));

    let tab_busy = codeterm_service_core::service::delivery_error::DeliveryError::TabBusyRefused {
        tab: "53854ac8".into(),
        detail: "x".into(),
    }
    .to_json();
    assert!(relay_error_summary(409, &tab_busy).starts_with(delivery::TAB_BUSY_REFUSED));

    let tab_unreachable =
        codeterm_service_core::service::delivery_error::DeliveryError::TabNotReachableOnPeer {
            tab: "ghost".into(),
        }
        .to_json();
    assert!(
        relay_error_summary(404, &tab_unreachable).starts_with(delivery::TAB_NOT_REACHABLE_ON_PEER)
    );
}

/// Keep the shared constants and the delivery error taxonomy aligned.
#[test]
fn shared_delivery_constants_match_the_delivery_error_taxonomy() {
    use codeterm_mesh::contract::delivery;
    use codeterm_service_core::service::delivery_error::DeliveryError;

    let tab = || "e35319d7".to_string();
    let detail = || String::new();
    assert_eq!(
        DeliveryError::TabNotReachableOnPeer { tab: tab() }.code(),
        delivery::TAB_NOT_REACHABLE_ON_PEER
    );
    assert_eq!(
        DeliveryError::TabBusyRefused {
            tab: tab(),
            detail: detail()
        }
        .code(),
        delivery::TAB_BUSY_REFUSED
    );
    assert_eq!(
        DeliveryError::PtyUnavailable {
            tab: tab(),
            detail: detail()
        }
        .code(),
        delivery::PTY_UNAVAILABLE
    );
    assert_eq!(
        DeliveryError::SubmitUnconfirmed {
            tab: tab(),
            detail: detail()
        }
        .code(),
        delivery::SUBMIT_UNCONFIRMED
    );
}
