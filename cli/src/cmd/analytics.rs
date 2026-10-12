use crate::{
    args::analytics::AnalyticsAction,
    cmd::print::{fail, fail_http, print_human},
};

pub fn handle(json_mode: bool, action: AnalyticsAction) {
    let client = crate::build_local_api().unwrap_or_else(|error| fail(1, error.to_string()));
    let request = match action {
        AnalyticsAction::Status => client.get("/api/analytics/status"),
        AnalyticsAction::Enable => client
            .post("/api/analytics/consent")
            .json(&serde_json::json!({"consent":"on"})),
        AnalyticsAction::Disable => client
            .post("/api/analytics/consent")
            .json(&serde_json::json!({"consent":"off"})),
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
    let value: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|_| fail(1, "invalid analytics status response"));
    if json_mode {
        println!("{value}");
    } else {
        print_human(&value);
    }
}

#[cfg(test)]
mod tests {
    use crate::{Cli, Commands};
    use clap::Parser;
    #[test]
    fn analytics_actions_parse_and_are_local() {
        for action in ["status", "enable", "disable"] {
            let cli = Cli::try_parse_from(["codeterm", "analytics", action]).unwrap();
            assert!(matches!(cli.command, Commands::Analytics { .. }));
            assert_eq!(
                crate::transport::transport_for(&cli.command).local,
                crate::transport::Local::PreSocket
            );
        }
    }
}
