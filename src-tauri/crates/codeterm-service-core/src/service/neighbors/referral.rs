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

#[cfg(test)]
mod tests;
