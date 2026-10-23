pub(crate) mod key_set;

use serde_json::Value;

use crate::client::{SocketCallError, SocketClient};
use crate::MemEmbedderAction;

const REFUSED: i32 = -32009;

pub(crate) fn method_and_params(action: &MemEmbedderAction) -> (&'static str, Option<Value>) {
    match action {
        MemEmbedderAction::Key { .. } => unreachable!("key set is handled before socket discovery"),
        MemEmbedderAction::List => ("mem.embedder.list", None),
        MemEmbedderAction::Status => ("mem.embedder.status", None),
        MemEmbedderAction::Use {
            provider,
            model,
            dimension,
        } => (
            "mem.embedder.use",
            Some(
                serde_json::json!({ "provider": provider, "model": model, "dimension": dimension }),
            ),
        ),
    }
}

pub(crate) fn handle(client: &mut SocketClient, json_mode: bool, action: MemEmbedderAction) {
    let (method, params) = method_and_params(&action);
    let value = match client.call_detailed(method, params) {
        Ok(value) => value,
        Err(SocketCallError::Rpc(error)) if error.code == REFUSED => {
            let hints: Vec<String> = error
                .data
                .as_ref()
                .and_then(|data| data["hints"].as_array())
                .into_iter()
                .flatten()
                .filter_map(|hint| hint.as_str().map(str::to_owned))
                .collect();
            let hints: Vec<&str> = hints.iter().map(String::as_str).collect();
            crate::cmd::print::fail_with_hints(2, error.message, &hints);
        }
        Err(error) => crate::cmd::print::fail(1, error.to_string()),
    };
    if json_mode {
        println!(
            "{}",
            serde_json::to_string_pretty(&value).unwrap_or_default()
        );
        return;
    }
    let text = match action {
        MemEmbedderAction::Key { .. } => unreachable!("key set is local-only"),
        MemEmbedderAction::List => render_list(&value),
        MemEmbedderAction::Status => render_status(&value),
        MemEmbedderAction::Use { .. } => render_use(&value),
    };
    print!("{text}");
}

fn str_of<'v>(value: &'v Value, key: &str) -> &'v str {
    value[key].as_str().unwrap_or("")
}

pub(crate) fn render_list(value: &Value) -> String {
    let mut out = String::new();
    for provider in value["providers"].as_array().into_iter().flatten() {
        let marker = if provider["active"].as_bool() == Some(true) {
            "*"
        } else {
            " "
        };
        out.push_str(&format!(
            "{marker} {}  {}\n",
            str_of(provider, "id"),
            str_of(provider, "label")
        ));
        for model in provider["models"].as_array().into_iter().flatten() {
            let dimension = model["dimension"].as_u64().unwrap_or(0);
            let min = model["min_dimension"].as_u64().unwrap_or(dimension);
            let dims = if min == dimension {
                format!("{dimension} dims")
            } else {
                format!("{dimension} dims ({min}..{dimension})")
            };
            let default = if model["default"].as_bool() == Some(true) {
                "  (default)"
            } else {
                ""
            };
            out.push_str(&format!(
                "    {}  {dims}  {} tokens{default}\n",
                str_of(model, "model"),
                model["max_tokens"].as_u64().unwrap_or(0)
            ));
        }
        if let Some(refusal) = provider.get("refusal").filter(|r| !r.is_null()) {
            out.push_str(&format!(
                "    unavailable: {}\n",
                str_of(refusal, "message")
            ));
            for hint in refusal["hints"].as_array().into_iter().flatten() {
                out.push_str(&format!("    hint: {}\n", hint.as_str().unwrap_or("")));
            }
        }
    }
    out
}

fn progress_line(reembed: &Value) -> String {
    let (done, total, failed) = (
        reembed["done"].as_u64().unwrap_or(0),
        reembed["total"].as_u64().unwrap_or(0),
        reembed["failed"].as_u64().unwrap_or(0),
    );
    match str_of(reembed, "state") {
        "running" => format!(
            "running {done}/{total} ({}%), {failed} failed",
            (done * 100).checked_div(total).unwrap_or(100)
        ),
        "completed" => format!("completed {done}/{total}, {failed} failed"),
        "failed" | "cancelled" => format!(
            "{} {done}/{total}: {}",
            str_of(reembed, "state"),
            str_of(reembed, "last_error")
        ),
        _ => "idle".into(),
    }
}

pub(crate) fn render_status(value: &Value) -> String {
    let index = &value["index"];
    let index = if index.is_null() {
        "none".to_string()
    } else {
        format!(
            "{}/{} ({} dims){}",
            str_of(index, "provider"),
            str_of(index, "model"),
            index["dimension"].as_u64().unwrap_or(0),
            if value["index_matches"].as_bool() == Some(true) {
                ""
            } else {
                "  — differs from the active embedder"
            }
        )
    };
    let remote = if value["remote"].as_bool() == Some(true) {
        " (remote)"
    } else {
        ""
    };
    format!(
        "provider   {}{remote}\nmodel      {}\ndimension  {}\navailable  {}\nindex      {index}\nvectors    {}\npending    {}\nqueued     {} chunks\nin flight  {}\nfailed     {}\nqueue      {}\nre-embed   {}\n",
        str_of(value, "provider"),
        str_of(value, "model"),
        value["dimension"].as_u64().unwrap_or(0),
        if value["available"].as_bool() == Some(true) { "yes" } else { "no" },
        value["vectors"].as_u64().unwrap_or(0),
        value["pending"].as_u64().unwrap_or(0),
        value["queue"]["queued"].as_u64().unwrap_or(0),
        value["queue"]["in_flight"].as_u64().unwrap_or(0),
        value["queue"]["failed"].as_u64().unwrap_or(0),
        if value["queue"]["unavailable"].as_bool() == Some(true) { "unavailable" } else { "available" },
        progress_line(&value["reembed"]),
    )
}

pub(crate) fn render_use(value: &Value) -> String {
    let target = &value["target"];
    if value["already_active"].as_bool() == Some(true) {
        let mut out = format!(
            "Embedder {}/{} is already active.\n",
            str_of(target, "provider"),
            str_of(target, "model")
        );
        let cancelled = &value["cancelled"];
        if !cancelled.is_null() {
            out.push_str(&format!(
                "Cancelled the re-embed to {}/{}.\n",
                str_of(cancelled, "provider"),
                str_of(cancelled, "model")
            ));
        }
        return out;
    }
    let plan = &value["plan"];
    let mut out = format!(
        "Preparing the embedder switch to {}/{} ({} dims).\nRe-embedding {} items in the background (items {}, entities {}, docs {}).\n",
        str_of(target, "provider"),
        str_of(target, "model"),
        target["dimension"].as_u64().unwrap_or(0),
        value["total"].as_u64().unwrap_or(0),
        plan["items"].as_u64().unwrap_or(0),
        plan["entities"].as_u64().unwrap_or(0),
        plan["docs"].as_u64().unwrap_or(0),
    );
    if let Some(warning) = value["warning"].as_str() {
        out.push_str(&format!("WARNING: {warning}\n"));
    }
    out.push_str("hint: follow progress with `domios mem embedder status`\n");
    out
}

#[cfg(test)]
mod tests;
