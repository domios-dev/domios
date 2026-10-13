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
pub(super) fn run(
    client: &crate::local_api::LocalApiClient,
    base: &str,
    json_output: bool,
    tab: String,
    device: bool,
    api_key_stdin: bool,
    cancel: bool,
) {
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
    let response = client
        .post(format!(
            "{base}/api/tabs/{}/agent/runtime/login",
            urlencoding::encode(&tab)
        ))
        .json(&request)
        .send();
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
    if json_output {
        println!("{value}");
        return;
    }
    println!(
        "Sign-in {}",
        value
            .pointer("/login/status")
            .and_then(|value| value.as_str())
            .unwrap_or("requested")
    );
    if let Some(url) = value.pointer("/login/url").and_then(|value| value.as_str()) {
        println!("Open in your system browser: {url}");
    }
    if let Some(code) = value
        .pointer("/login/userCode")
        .and_then(|value| value.as_str())
    {
        println!("Device code: {code}");
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn login_cli_accepts_stdin_and_refuses_argv_credentials() {
        use clap::Parser;
        for flags in [
            vec![],
            vec!["--device"],
            vec!["--api-key-stdin"],
            vec!["--cancel"],
        ] {
            let mut args = vec!["codeterm", "agent", "login", "--tab", "fixture"];
            args.extend(flags);
            assert!(crate::args::cli::Cli::try_parse_from(args).is_ok());
        }
        assert!(crate::args::cli::Cli::try_parse_from([
            "codeterm",
            "agent",
            "login",
            "--tab",
            "fixture",
            "--api-key",
            "fixture"
        ])
        .is_err());
        assert!(crate::args::cli::Cli::try_parse_from([
            "codeterm",
            "agent",
            "login",
            "--tab",
            "fixture",
            "--api-key-stdin",
            "--device"
        ])
        .is_err());
    }
    #[test]
    fn api_key_input_is_bounded_and_trimmed() {
        assert_eq!(
            read_key(&b" opaque-fixture\n"[..]).unwrap(),
            "opaque-fixture"
        );
        assert!(read_key(&b"\n"[..]).is_err());
        assert!(read_key(vec![b'x'; 16385].as_slice()).is_err());
    }
}
