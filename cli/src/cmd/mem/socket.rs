use crate::client::SocketClient;
use crate::MemAction;

pub(super) fn try_dispatch(
    client: &mut SocketClient,
    action: &MemAction,
) -> Option<Result<serde_json::Value, String>> {
    let (method, params) = socket_request(action)?;
    Some(client.call(method, params))
}

fn socket_request(action: &MemAction) -> Option<(&'static str, Option<serde_json::Value>)> {
    match action {
        MemAction::Stats => Some(("mem.stats", None)),
        MemAction::Search {
            query,
            kind,
            area,
            scope,
            project,
            limit,
            rerank,
        } => {
            let project = project.clone().or_else(|| {
                std::env::current_dir()
                    .ok()
                    .map(|path| path.display().to_string())
            });
            Some((
                "mem.search",
                Some(serde_json::json!({
                    "q": query,
                    "kind": kind,
                    "area": area,
                    "scope": scope,
                    "project": project,
                    "limit": limit,
                    "rerank": rerank,
                })),
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::socket_request;
    use crate::{Cli, Commands, MemAction};
    use clap::Parser;

    #[test]
    fn observed_issue_actions_are_socket_routed() {
        for args in [
            vec!["codeterm", "mem", "stats"],
            vec!["codeterm", "mem", "search", "test", "--limit", "1"],
        ] {
            let cli = Cli::try_parse_from(args).unwrap();
            let Commands::Mem { action } = cli.command else {
                panic!("expected mem command");
            };
            let (method, params) = socket_request(&action).expect("socket-routed mem action");
            match action {
                MemAction::Stats => {
                    assert_eq!(method, "mem.stats");
                    assert!(params.is_none());
                }
                MemAction::Search { .. } => {
                    assert_eq!(method, "mem.search");
                    let params = params.unwrap();
                    assert_eq!(params["q"], "test");
                    assert_eq!(params["limit"], 1);
                }
                _ => panic!("unexpected mem action"),
            }
        }
    }
}
