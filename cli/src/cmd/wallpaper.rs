use serde_json::{json, Value};

use crate::WallpaperAction;

pub(crate) fn request(action: &WallpaperAction) -> (&'static str, String, Value) {
    match action {
        WallpaperAction::Set { from_pad, request } => {
            let svg = super::payload_source::read_bounded_from_pad(
                from_pad,
                codeterm_types::svg::MAX_CHAT_TILE_BYTES,
            )
            .unwrap_or_else(|error| super::print::fail(1, error));
            let path = request.map_or_else(
                || "/api/chat-wallpaper/apply-svg".into(),
                |id| format!("/api/chat-wallpaper/drafts/{id}"),
            );
            ("POST", path, json!({ "svg": svg }))
        }
        WallpaperAction::Clear => (
            "PUT",
            "/api/chat-wallpaper".into(),
            json!({ "wallpaper": null }),
        ),
    }
}

pub(crate) fn handle(json_output: bool, action: &WallpaperAction) {
    let (method, path, body) = request(action);
    let http = crate::build_local_api().unwrap_or_else(|error| {
        super::print::fail(1, format!("failed to connect to local API: {error}"))
    });
    let request = match method {
        "POST" => http.post(path),
        _ => http.put(path),
    };
    let response = request
        .json(&body)
        .send()
        .unwrap_or_else(|error| super::print::fail(1, error.to_string()));
    if !response.status().is_success() {
        super::print::fail_http(
            1,
            response.status().as_u16(),
            &response.text().unwrap_or_default(),
        );
    }
    let result: Value = response
        .json()
        .unwrap_or_else(|error| super::print::fail(1, error.to_string()));
    if json_output {
        super::print::print_json(&result);
    } else if let Some(id) = result["id"].as_str() {
        println!("Wallpaper applied: {id}");
    } else {
        println!("Theme wallpaper restored");
    }
}
