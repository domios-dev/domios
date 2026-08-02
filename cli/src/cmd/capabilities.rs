use serde_json::Value;

fn rpc_only_replacement(method: &str) -> Option<Option<&'static str>> {
    match method {
        "agent.children" | "agent.hierarchy" => Some(Some("codeterm agent ledger")),
        "tab.send_text" => Some(Some("codeterm send")),
        "tab.get_output" => Some(Some("codeterm tab get-text --tab <ID>")),
        "workspace.recently_closed"
        | "workspace.reopen_closed"
        | "workspace.reopen_closed_group"
        | "tab.resize"
        | "tab.kill_pty"
        | "agent.set_role"
        | "agent.clear_role"
        | "agent.link"
        | "plan.binding.resolve"
        | "plan.batch" => Some(None),
        _ => None,
    }
}

fn cli_command(method: &str) -> String {
    format!("codeterm {}", method.replace('.', " ").replace('_', "-"))
}

pub fn render(value: &Value) -> String {
    let Some(methods) = value.as_array() else {
        return "No capabilities reported.\n".into();
    };
    let mut out = String::new();
    for method in methods.iter().filter_map(Value::as_str) {
        match rpc_only_replacement(method) {
            Some(Some(replacement)) => {
                out.push_str(&format!("{method}  [RPC only; use `{replacement}`]\n"));
            }
            Some(None) => out.push_str(&format!("{method}  [RPC only/internal]\n")),
            None => out.push_str(&format!("{method}  [CLI: `{}`]\n", cli_command(method))),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn capabilities_mark_rpc_only_and_name_cli_replacements() {
        let out = render(&json!(["mem.search", "agent.children", "tab.resize"]));
        assert!(out.contains("mem.search  [CLI: `codeterm mem search`]"));
        assert!(out.contains("agent.children  [RPC only; use `codeterm agent ledger`]"));
        assert!(out.contains("tab.resize  [RPC only/internal]"));
    }
}
