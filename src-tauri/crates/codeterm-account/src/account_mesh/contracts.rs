use super::AccountMeshRefusal;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(untagged)]
pub enum AccountMeshOutcome<T> {
    Done {
        ok: bool,
        result: T,
    },
    Refused {
        ok: bool,
        request_id: Option<String>,
        failure: AccountMeshRefusal,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        #[ts(as = "Option<bool>", optional)]
        local_signed_out: bool,
    },
}

impl<T> AccountMeshOutcome<T> {
    pub fn of(result: Result<T, AccountMeshRefusal>, request_id: Option<String>) -> Self {
        match result {
            Ok(result) => Self::Done { ok: true, result },
            Err(failure) => Self::Refused {
                ok: false,
                request_id,
                failure,
                local_signed_out: false,
            },
        }
    }
}
