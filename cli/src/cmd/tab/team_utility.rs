use crate::cmd::print::{handle_result, print_human};
use crate::{build_local_api, read_api_config};

pub(super) fn handle_create_result(
    json: bool,
    team_id: Option<&str>,
    result: Result<serde_json::Value, String>,
) {
    if json || team_id.is_none() || result.is_err() {
        handle_result(json, result);
        return;
    }

    let value = result.unwrap();
    print_human(&value);
    let team_id = team_id.unwrap();
    match fetch_live_tabs() {
        Ok(tabs) => {
            if let Some(summary) = render_team_utility_shells(&tabs, team_id) {
                println!("\n{summary}");
            }
        }
        Err(error) => {
            eprintln!("warning: tab created, but Team utility shells could not be listed: {error}")
        }
    }
}

fn fetch_live_tabs() -> Result<Vec<serde_json::Value>, String> {
    let (base, _) = read_api_config();
    let api = build_local_api().map_err(|error| error.to_string())?;
    let response = api
        .get(format!("{base}/api/tabs?all_workspaces=true"))
        .send()
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let text = response.text().unwrap_or_default();
    if !status.is_success() {
        return Err(format!("HTTP {}: {text}", status.as_u16()));
    }
    serde_json::from_str::<serde_json::Value>(&text)
        .map_err(|error| error.to_string())?
        .as_array()
        .cloned()
        .ok_or_else(|| "tab list response is not an array".to_string())
}

fn render_team_utility_shells(tabs: &[serde_json::Value], team_id: &str) -> Option<String> {
    let mut shells = tabs
        .iter()
        .filter(|tab| string_field(tab, "team_id") == team_id)
        .filter(|tab| string_field(tab, "role").is_empty())
        .filter(|tab| string_field(tab, "chat_backend_plugin_id").is_empty())
        .filter_map(|tab| {
            let id = string_field(tab, "id");
            if id.is_empty() {
                return None;
            }
            let title = ["tab_title", "title"]
                .into_iter()
                .map(|field| string_field(tab, field))
                .find(|value| !value.is_empty())
                .unwrap_or("shell");
            Some((title.to_string(), id.to_string()))
        })
        .collect::<Vec<_>>();
    if shells.is_empty() {
        return None;
    }
    shells.sort();

    let mut lines = vec![format!("Team utility shells ({}):", shells.len())];
    lines.extend(shells.iter().map(|(title, id)| format!("  {id} {title}")));
    if shells.len() > 1 {
        lines.push("Clean up extras with `codeterm tab close --tab <ID>`.".to_string());
    }
    Some(lines.join("\n"))
}

fn string_field<'a>(value: &'a serde_json::Value, field: &str) -> &'a str {
    value
        .get(field)
        .and_then(|item| item.as_str())
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn summary_lists_only_live_utility_shells_from_the_target_team() {
        let tabs = vec![
            json!({"id":"shell-b","tab_title":"tests","team_id":"team-a"}),
            json!({"id":"manager","tab_title":"Ada","team_id":"team-a","role":"Manager"}),
            json!({"id":"shell-a","tab_title":"dev","team_id":"team-a"}),
            json!({"id":"plugin","tab_title":"LM Studio","team_id":"team-a","chat_backend_plugin_id":"lmstudio"}),
            json!({"id":"foreign","tab_title":"other","team_id":"team-b"}),
        ];

        let output = render_team_utility_shells(&tabs, "team-a").unwrap();

        assert_eq!(
            output,
            "Team utility shells (2):\n  shell-a dev\n  shell-b tests\nClean up extras with `codeterm tab close --tab <ID>`."
        );
    }

    #[test]
    fn single_team_shell_does_not_claim_that_cleanup_is_needed() {
        let tabs = vec![json!({"id":"shell","title":"terminal","team_id":"team"})];

        let output = render_team_utility_shells(&tabs, "team").unwrap();

        assert_eq!(output, "Team utility shells (1):\n  shell terminal");
    }
}
