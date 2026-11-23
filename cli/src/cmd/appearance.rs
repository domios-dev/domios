use serde_json::{json, Value};

use crate::AppearanceAction;

use super::print::{fail, fail_http, print_json};

pub(crate) fn request(action: &AppearanceAction) -> (&'static str, String, Option<Value>) {
    let endpoint = |machine: &str| format!("/api/appearance/{}", urlencoding::encode(machine));
    let proposal =
        |machine: &str, id: &str| format!("{}/{}", endpoint(machine), urlencoding::encode(id));
    match action {
        AppearanceAction::Current { machine } => ("GET", endpoint(machine), None),
        AppearanceAction::Tweak {
            machine,
            revision,
            from_pad,
        } => {
            let toml = super::payload_source::read_bounded_from_pad(from_pad, 4096)
                .unwrap_or_else(|error| fail(1, error));
            (
                "POST",
                format!("{}/tweak", endpoint(machine)),
                Some(json!({ "revision": revision, "toml": toml })),
            )
        }
        AppearanceAction::Propose {
            machine,
            from_pad,
            rationale,
        } => {
            let toml = super::payload_source::read_bounded_from_pad(from_pad, 256 * 1024)
                .unwrap_or_else(|error| fail(1, error));
            (
                "POST",
                endpoint(machine),
                Some(json!({ "toml": toml, "rationale": rationale })),
            )
        }
        AppearanceAction::Show { machine, id } => ("GET", proposal(machine, id), None),
        AppearanceAction::Apply { machine, id } => {
            ("POST", format!("{}/apply", proposal(machine, id)), None)
        }
        AppearanceAction::Undo { machine, id } => {
            ("POST", format!("{}/undo", proposal(machine, id)), None)
        }
    }
}

pub fn handle(json: bool, action: &AppearanceAction) {
    let client = crate::build_local_api().unwrap_or_else(|error| fail(1, error.to_string()));
    let (method, path, body) = request(action);
    let response = if method == "GET" {
        client.get(&path).send()
    } else {
        client.post(&path).json(&body).send()
    }
    .unwrap_or_else(|error| fail(1, error.to_string()));
    if !response.status().is_success() {
        fail_http(
            1,
            response.status().as_u16(),
            &response.text().unwrap_or_default(),
        );
    }
    let value: Value = response
        .json()
        .unwrap_or_else(|error| fail(1, error.to_string()));
    if json {
        print_json(&value);
    } else {
        if let (Some(id), Some(machine)) = (value["id"].as_str(), value["machine"]["id"].as_str()) {
            println!("<ds-appearance id=\"{id}\" machine=\"{machine}\" />");
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&value).expect("JSON value serializes")
        );
    }
}
