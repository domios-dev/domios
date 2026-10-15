pub enum DeepLinkAction {
    BillingReturn { status: Option<String> },
    AccountCallback(codeterm_account::Callback),
}

pub fn deep_link_action(url: &url::Url) -> Option<DeepLinkAction> {
    if url.scheme() == "domios" && url.host_str() == Some("billing") {
        if url.path() != "/return"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.fragment().is_some()
        {
            return None;
        }
        let mut status = None;
        for (key, value) in url.query_pairs() {
            if key != "status"
                || status.is_some()
                || value.is_empty()
                || value.len() > 64
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
            {
                return None;
            }
            status = Some(value.into_owned());
        }
        return Some(DeepLinkAction::BillingReturn { status });
    }
    codeterm_account::Callback::parse(url.as_str())
        .ok()
        .map(DeepLinkAction::AccountCallback)
}

#[cfg(test)]
mod tests;
