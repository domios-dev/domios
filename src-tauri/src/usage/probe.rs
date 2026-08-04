use std::time::Instant;

use crate::service::ServiceContext;

#[derive(Debug, Clone)]
pub struct UsageProbeOutcome {
    pub raw_text: String,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone)]
pub enum ProbeError {
    BinaryMissing(String),
    FetchFailed(String),
    /// The provider exposes no way to read its meter without moving it. There is
    /// deliberately no metered fallback: telemetry must never bill the user.
    NoQuotaFreeSource(String),
    Internal(String),
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BinaryMissing(p) => write!(f, "binary missing: {}", p),
            Self::FetchFailed(m) => write!(f, "usage fetch failed: {}", m),
            Self::NoQuotaFreeSource(p) => write!(
                f,
                "{} exposes no quota-free usage source; refusing to spend quota to read it",
                p
            ),
            Self::Internal(m) => write!(f, "internal: {}", m),
        }
    }
}

impl std::error::Error for ProbeError {}

pub struct UsageProbe {
    svc: ServiceContext,
}

impl UsageProbe {
    pub fn new(svc: ServiceContext) -> Self {
        Self { svc }
    }

    pub async fn run(
        &self,
        provider: &str,
        _account_id: Option<&str>,
    ) -> Result<UsageProbeOutcome, ProbeError> {
        let started = Instant::now();
        let provider_owned = provider.to_string();
        let svc = self.svc.clone();

        let registry_has = svc
            .agent_registry
            .with_provider(&provider_owned, |_| ())
            .is_some();
        if !registry_has {
            return Err(ProbeError::BinaryMissing(provider_owned));
        }

        let svc_for_fetch = svc.clone();
        let provider_for_fetch = provider_owned.clone();
        let fetched = tokio::task::spawn_blocking(move || {
            svc_for_fetch
                .agent_registry
                .with_provider(&provider_for_fetch, |p| p.fetch_usage())
                .flatten()
        })
        .await
        .map_err(|e| ProbeError::Internal(format!("fetch join: {}", e)))?;

        quota_free_outcome(
            fetched,
            &provider_owned,
            started.elapsed().as_millis() as u64,
        )
    }
}

/// The provider's own fetch path is the ONLY usage source. `None` means the
/// provider has no quota-free source, which is an honest failure — never a
/// reason to escalate to something that spends the meter being measured.
fn quota_free_outcome(
    fetched: Option<Result<String, String>>,
    provider: &str,
    elapsed_ms: u64,
) -> Result<UsageProbeOutcome, ProbeError> {
    match fetched {
        Some(Ok(raw_text)) => Ok(UsageProbeOutcome {
            raw_text,
            elapsed_ms,
        }),
        Some(Err(e)) => Err(ProbeError::FetchFailed(e)),
        None => Err(ProbeError::NoQuotaFreeSource(provider.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::{quota_free_outcome, ProbeError};

    #[test]
    fn missing_fetch_source_fails_instead_of_spending_quota() {
        let err = quota_free_outcome(None, "tui-only-provider", 0).expect_err("must not succeed");
        assert!(
            matches!(err, ProbeError::NoQuotaFreeSource(ref p) if p == "tui-only-provider"),
            "a provider with no quota-free source must fail honestly, got {:?}",
            err
        );
    }

    #[test]
    fn fetched_payload_is_returned_verbatim() {
        let out = quota_free_outcome(Some(Ok("5h limit 64% left".into())), "fetch-provider", 7)
            .expect("fetch payload is an outcome");
        assert_eq!(out.raw_text, "5h limit 64% left");
        assert_eq!(out.elapsed_ms, 7);
    }

    #[test]
    fn fetch_failure_stays_a_fetch_failure() {
        let err = quota_free_outcome(Some(Err("no token".into())), "fetch-provider", 0)
            .expect_err("must not succeed");
        assert!(
            matches!(err, ProbeError::FetchFailed(ref m) if m == "no token"),
            "a failing fetch must not be reported as a missing source, got {:?}",
            err
        );
    }
}
