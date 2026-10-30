use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    Granted,
    AlreadyGranted,
    NotInvited,
    InviterNotPaid,
    AlreadyPaid,
    NotFresh,
    InviteExpired,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Claim {
    #[serde(rename = "schema_version")]
    _schema_version: codeterm_billing::Version<1>,
    outcome: Outcome,
    trial_end: Option<u64>,
    friend_request_id: Option<String>,
}

pub(super) fn refresh_required(value: &serde_json::Value) -> Result<bool, String> {
    let claim: Claim = serde_json::from_value(value.clone())
        .map_err(|_| "Invalid referral response".to_owned())?;
    let granted = matches!(claim.outcome, Outcome::Granted | Outcome::AlreadyGranted);
    if granted != claim.trial_end.is_some_and(|end| end > 0)
        || (!granted && claim.trial_end.is_some())
        || claim.friend_request_id.as_ref().is_some_and(|id| {
            id.len() != 64
                || !id
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
    {
        return Err("Invalid referral response".into());
    }
    Ok(granted)
}

pub(super) fn no_claim() -> serde_json::Value {
    serde_json::json!({"schema_version":1,"outcome":"not_invited","trial_end":null,"friend_request_id":null})
}

pub(super) async fn claim_once(
    metadata: &super::storage::Metadata,
    load: impl std::future::Future<Output = codeterm_neighbors::Result<serde_json::Value>>,
) -> codeterm_neighbors::Result<serde_json::Value> {
    if let Some(receipt) = metadata.read::<serde_json::Value>("referral-claim.json".into()).await? {
        refresh_required(&receipt).map_err(|_| codeterm_neighbors::Error::InvalidEnvelope)?;
        return Ok(receipt);
    }
    metadata.write("referral-claim.json".into(), no_claim()).await?;
    let result = load.await?;
    refresh_required(&result).map_err(|_| codeterm_neighbors::Error::InvalidEnvelope)?;
    metadata.write("referral-claim.json".into(), result.clone()).await?;
    Ok(result)
}

#[cfg(test)]
mod tests;
