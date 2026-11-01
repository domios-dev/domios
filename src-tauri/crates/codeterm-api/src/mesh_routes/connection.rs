use axum::http::StatusCode;
use codeterm_service_core::mesh::account_mesh::AdmissionRefusal;

pub(crate) fn refusal_status(refusal: &AdmissionRefusal) -> StatusCode {
    match refusal {
        AdmissionRefusal::MachineLimit { .. } | AdmissionRefusal::PlanLimitLocked => {
            StatusCode::PAYMENT_REQUIRED
        }
        AdmissionRefusal::SignedOut | AdmissionRefusal::Unauthenticated => StatusCode::UNAUTHORIZED,
        AdmissionRefusal::Revoked => StatusCode::FORBIDDEN,
        AdmissionRefusal::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
        AdmissionRefusal::Offline | AdmissionRefusal::PeerUnreachable => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        AdmissionRefusal::Cancelled => StatusCode::CONFLICT,
        AdmissionRefusal::HandshakeFailed { .. } => StatusCode::UNPROCESSABLE_ENTITY,
    }
}
