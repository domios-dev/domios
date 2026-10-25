use serde::{Deserialize, Serialize};

pub const APPROVAL_TTL_SECS: u64 = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkDecision {
    Waiting,
    Approved,
    Declined,
    Expired,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinkRequest {
    pub notification_id: Option<String>,
    pub request_id: String,
    pub installation_id: String,
    pub name: String,
    pub incoming: bool,
    pub expires_at: u64,
    pub decision: LinkDecision,
    pub failure: Option<super::AdmissionRefusal>,
}

impl LinkRequest {
    pub fn decision_at(&self, now: u64) -> LinkDecision {
        if self.incoming && self.decision == LinkDecision::Waiting && now >= self.expires_at {
            LinkDecision::Expired
        } else {
            self.decision
        }
    }
}

pub async fn wait_for_link_approval(
    mesh: &crate::MeshState,
    request_id: &str,
) -> Result<(), super::AdmissionRefusal> {
    let mut changes = mesh.lock().gate_watch();
    let wait = async {
        loop {
            if mesh.lock().link_approval_result(request_id)? {
                return Ok(());
            }
            changes
                .changed()
                .await
                .map_err(|_| super::AdmissionRefusal::Cancelled)?;
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(APPROVAL_TTL_SECS), wait)
        .await
        .unwrap_or(Err(super::AdmissionRefusal::Expired))
}
