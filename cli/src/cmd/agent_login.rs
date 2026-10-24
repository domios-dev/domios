use serde_json::json;
use std::io::Read;

fn read_key(input: impl Read) -> Result<String, String> {
    let mut key = String::new();
    input
        .take(16385)
        .read_to_string(&mut key)
        .map_err(|_| "Could not read API key from stdin")?;
    if key.len() > 16384 {
        return Err("API key input exceeds 16 KiB".into());
    }
    let key = key.trim().to_string();
    if key.is_empty() || key.len() > 16384 {
        return Err("API key must contain 1–16384 bytes".into());
    }
    Ok(key)
}
pub(super) struct LoginOptions {
    pub device: bool,
    pub api_key_stdin: bool,
    pub cancel: bool,
    pub show: bool,
}
pub(super) fn run(
    client: &crate::local_api::LocalApiClient,
    base: &str,
    json_output: bool,
    tab: String,
    options: LoginOptions,
) {
    let LoginOptions {
        device,
        api_key_stdin,
        cancel,
        show,
    } = options;
    let mut request = if cancel {
        json!({"action":"cancel"})
    } else {
        json!({"action":"start","method":if device {"device"} else if api_key_stdin {"apiKey"} else {"browser"}})
    };
    if api_key_stdin {
        match read_key(std::io::stdin().lock()) {
            Ok(key) => request["apiKey"] = key.into(),
            Err(error) => crate::cmd::print::fail(1, error),
        }
    }
    let endpoint = format!(
        "{base}/api/tabs/{}/agent/runtime/login",
        urlencoding::encode(&tab)
    );
    let response = if show {
        client.get(endpoint).send()
    } else {
        client.post(endpoint).json(&request).send()
    };
    let response = match response {
        Ok(response) => response,
        Err(_) => crate::cmd::print::fail(1, "Sign-in request could not reach the daemon"),
    };
    if !response.status().is_success() {
        crate::cmd::print::fail(1, "Sign-in request failed. Check the tab's sign-in banner.");
    }
    let value: serde_json::Value = match response.json() {
        Ok(value) => value,
        Err(_) => crate::cmd::print::fail(1, "Invalid sign-in response"),
    };
    print!("{}", render(&value, json_output));
}

fn render(value: &serde_json::Value, json_output: bool) -> String {
    use std::fmt::Write;
    let mut output = String::new();
    if json_output {
        return format!("{value}\n");
    }
    let _ = writeln!(
        output,
        "Sign-in {}",
        value
            .pointer("/login/status")
            .and_then(|value| value.as_str())
            .unwrap_or("requested")
    );
    if let Some(url) = value.pointer("/login/url").and_then(|value| value.as_str()) {
        let _ = writeln!(output, "Open in your system browser: {url}");
    }
    if let Some(code) = value
        .pointer("/login/userCode")
        .and_then(|value| value.as_str())
    {
        let _ = writeln!(output, "Device code: {code}");
    }
    if let Some(expiry) = value.pointer("/login/expiresAtMs").and_then(|v| v.as_u64()) {
        let _ = writeln!(output, "Expires at (Unix milliseconds): {expiry}");
    } else if value.pointer("/login/status").and_then(|v| v.as_str()) == Some("pending") {
        let _ = writeln!(output, "Expiry: not reported by provider");
    }
    output
}
#[cfg(test)]
#[path = "agent_login/tests.rs"]
mod tests;
