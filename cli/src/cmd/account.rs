use crate::{
    args::account::AccountAction,
    cmd::print::{fail, fail_http, print_human},
};

mod login_paste;
mod neighbors;

pub fn handle(json_mode: bool, action: AccountAction) {
    let client = crate::build_local_api().unwrap_or_else(|error| fail(1, error.to_string()));
    let request = match action {
        AccountAction::Plan => client.get("/api/account/plan"),
        AccountAction::Upgrade { plan_key } => client
            .post("/api/account/upgrade")
            .json(&serde_json::json!({"plan":plan_key})),
        AccountAction::Portal => client.post("/api/account/portal"),
        AccountAction::Status => client.get("/api/account/status"),
        AccountAction::Login {
            no_browser: true,
            code_from_pad,
        } => return login_paste::handle(&client, json_mode, code_from_pad.as_deref()),
        AccountAction::Login { .. } => client.post("/api/account/login"),
        AccountAction::Logout => client.post("/api/account/logout"),
        AccountAction::Neighbors { action } => return neighbors::handle(json_mode, action),
    };
    let response = request
        .send()
        .unwrap_or_else(|error| fail(1, error.to_string()));
    let status = response.status();
    let body = response
        .text()
        .unwrap_or_else(|error| fail(1, error.to_string()));
    if !status.is_success() {
        fail_http(1, status.as_u16(), &body);
    }
    let value: serde_json::Value =
        serde_json::from_str(&body).unwrap_or_else(|_| fail(1, "invalid account status response"));
    if json_mode {
        println!("{value}");
    } else {
        print_human(&value);
    }
}

#[cfg(test)]
mod tests;
