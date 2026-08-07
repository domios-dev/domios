use crate::client::SocketClient;
use crate::cmd::print::handle_result;

use super::relay_tab_json;
use super::remote_target::remote_inspect_target;

pub(super) fn handle_tab_inspect(
    client: &mut SocketClient,
    json: bool,
    mut tabs: Vec<String>,
    tab: Vec<String>,
    lifecycle: bool,
) {
    tabs.extend(tab);
    if tabs.is_empty() {
        eprintln!("Error: tab inspect requires --tab <ID>");
        std::process::exit(2);
    }
    if lifecycle && tabs.len() != 1 {
        eprintln!("Error: --lifecycle requires exactly one tab");
        std::process::exit(2);
    }
    match remote_inspect_target(&tabs) {
        Ok(Some((peer, remote_tabs))) => {
            let ids = remote_tabs
                .iter()
                .map(|tab| urlencoding::encode(tab).into_owned())
                .collect::<Vec<_>>()
                .join(",");
            let lifecycle_query = if lifecycle { "&lifecycle=true" } else { "" };
            let data = relay_tab_json(
                &peer,
                "GET",
                &format!("/api/tabs/inspect?tab_ids={ids}{lifecycle_query}"),
                None,
            );
            handle_result(json, Ok(data));
        }
        Err(error) => handle_result(json, Err(error)),
        Ok(None) => {
            let result = client.call(
                "tab.inspect",
                Some(serde_json::json!({ "tab_ids": tabs, "lifecycle": lifecycle })),
            );
            handle_result(json, result);
        }
    }
}
