use super::{identifier, invalid, RuntimeFailure, RuntimeLoginState};

pub(super) fn validate(login: &RuntimeLoginState) -> Result<(), RuntimeFailure> {
    if login
        .expires_at_ms
        .is_some_and(|expiry| expiry > 9_007_199_254_740_991)
        || login.login_id.as_ref().is_some_and(|id| !identifier(id))
        || login
            .instructions
            .as_ref()
            .is_some_and(|instructions| instructions.len() > 4096 || instructions.contains('\0'))
        || login
            .user_code
            .as_ref()
            .is_some_and(|code| !identifier(code))
        || login
            .error
            .as_ref()
            .is_some_and(|error| error.len() > 65536)
        || login.url.as_ref().is_some_and(|url| {
            url.len() > 16384 || !url.starts_with("https://") || url.contains(['\r', '\n', '\0'])
        })
    {
        return Err(invalid("invalid login event"));
    }
    Ok(())
}
