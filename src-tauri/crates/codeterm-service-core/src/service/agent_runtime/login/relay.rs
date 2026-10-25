use super::*;

pub(super) fn phone_body(login: &RuntimeLoginState) -> Result<String, String> {
    if login.status != RuntimeLoginStatus::Pending {
        return Err("No active sign-in to send".into());
    }
    let url = login.url.as_deref().ok_or("No sign-in link available")?;
    let mut body = url.to_string();
    if let Some(code) = &login.user_code {
        body.push_str(&format!("\nDevice code: {code}"));
    }
    if let Some(expiry) = login.expires_at_ms {
        body.push_str(&format!("\nExpires at (Unix milliseconds): {expiry}"));
    }
    Ok(body)
}

pub(super) fn notify(
    svc: &ServiceContext,
    tab: &str,
    session: &Arc<Session>,
) -> Result<Value, String> {
    let login =
        project(session.login.lock().login.as_ref(), now_ms()).ok_or("No pending sign-in")?;
    let body = phone_body(&login)?;
    if let Some(id) = session.login.lock().phone_notification.clone() {
        return Ok(json!({"notificationId":id}));
    }
    let id = svc.create_login_notification(tab, body)?;
    session.login.lock().phone_notification = Some(id.clone());
    Ok(json!({"notificationId":id}))
}

#[cfg(test)]
mod tests;
