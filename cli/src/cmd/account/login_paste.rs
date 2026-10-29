use crate::{
    cmd::print::{fail, print_human},
    local_api::LocalApiClient,
};
use std::io::{BufRead, Read, Write};

pub(super) fn handle(client: &LocalApiClient, json: bool, pad: Option<&str>) {
    let result = run(client, json, pad);
    match result {
        Ok(value) if json => println!("{value}"),
        Ok(value) => print_human(&value),
        Err(error) => fail(1, error),
    }
}

fn run(
    client: &LocalApiClient,
    json: bool,
    pad: Option<&str>,
) -> Result<serde_json::Value, String> {
    let request = if pad.is_some() {
        client.get("/api/account/login/paste")
    } else {
        client.post("/api/account/login/paste")
    };
    let start: serde_json::Value = request
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.json())
        .map_err(|e| e.to_string())?;
    let url = start["url"].as_str().ok_or("Sign-in response has no URL")?;
    let id = start["login_id"]
        .as_str()
        .ok_or("Sign-in response has no login ID")?;
    if json {
        println!("{}", serde_json::json!({"url": url, "pending": true}));
    } else {
        println!("{url}");
    }
    std::io::stdout().flush().map_err(|e| e.to_string())?;
    eprintln!("Open this URL on another machine, then paste the whole redirected callback URL (including code and state):");
    let callback = match pad {
        Some(path) => crate::cmd::payload_source::read_from_pad(path)?,
        None => {
            let mut value = String::new();
            if std::io::stdin()
                .lock()
                .take(4098)
                .read_line(&mut value)
                .map_err(|e| e.to_string())?
                == 0
            {
                return Ok(serde_json::json!({"pending": true}));
            }
            value
        }
    };
    client
        .put("/api/account/login/paste")
        .json(&serde_json::json!({"login_id": id, "url": callback.trim()}))
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.json())
        .map_err(|e| e.to_string())
}
