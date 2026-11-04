use serde_json::{json, Value};

pub(super) fn record(method: &str, params: &Value) -> Option<Value> {
    let bounded = |s: &str| s.chars().take(2000).collect::<String>();
    match method {
        "Runtime.consoleAPICalled" => {
            let text = params
                .get("args")?
                .as_array()?
                .iter()
                .take(20)
                .map(|arg| match arg.get("value") {
                    Some(Value::String(s)) => bounded(s),
                    Some(Value::Bool(b)) => b.to_string(),
                    Some(Value::Number(n)) => n.to_string(),
                    _ => "[object omitted]".into(),
                })
                .collect::<Vec<_>>()
                .join(" ");
            Some(
                json!({"kind":"console","level":bounded(params["type"].as_str().unwrap_or("log")),"text":bounded(&super::redaction::text(&text))}),
            )
        }
        "Network.requestWillBeSent" | "Network.responseReceived" => {
            let field = if method == "Network.requestWillBeSent" {
                "request"
            } else {
                "response"
            };
            let data = &params[field];
            let mut url = url::Url::parse(data["url"].as_str()?).ok()?;
            if !matches!(url.scheme(), "http" | "https") {
                return None;
            }
            let _ = url.set_username("");
            let _ = url.set_password(None);
            url.set_query(None);
            url.set_fragment(None);
            Some(
                json!({"kind":"network","event":field,"url":bounded(url.as_str()),"method":bounded(data["method"].as_str().unwrap_or("")),"status":data["status"].as_u64()}),
            )
        }
        _ => None,
    }
}

pub(super) fn parse(args: &[String], console: bool) -> Result<(Option<String>, usize), String> {
    let mut pattern = None;
    let mut limit = None;
    let mut words = args.iter();
    while let Some(flag) = words.next() {
        let value = words.next().ok_or("missing diagnostic option value")?;
        match flag.as_str() {
            "--pattern" if console && pattern.is_none() && value.len() <= 256 => {
                pattern = Some(value.clone())
            }
            "--limit" if limit.is_none() => {
                limit = Some(
                    value
                        .parse::<usize>()
                        .ok()
                        .filter(|n| (1..=200).contains(n))
                        .ok_or("limit must be 1 to 200")?,
                )
            }
            _ => return Err("unknown or duplicate diagnostic option".into()),
        }
    }
    Ok((pattern, limit.unwrap_or(100)))
}

#[cfg(test)]
mod tests;
