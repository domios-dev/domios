use super::BrowserRuntime;
use codeterm_browser::{
    snapshot::{
        build_snapshot_filtered, parse_full_ax_tree, query, sensitive, DEFAULT_MAX_LINES, MAX_LINES,
    },
    PageChannel,
};
use serde_json::json;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Options {
    pub json: bool,
    pub scope: Option<String>,
    pub actions: bool,
    pub limit: usize,
}

pub(super) fn parse(args: &[String]) -> Result<Options, String> {
    let mut out = Options {
        json: false,
        scope: None,
        actions: false,
        limit: DEFAULT_MAX_LINES,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => out.json = true,
            "--actions-only" => out.actions = true,
            "--scope" => {
                i += 1;
                out.scope = Some(args.get(i).ok_or("--scope requires a value")?.clone());
            }
            "--max-items" => {
                i += 1;
                out.limit = args
                    .get(i)
                    .ok_or("--max-items requires a number")?
                    .parse::<usize>()
                    .map_err(|_| "invalid max-items")?;
                if out.limit == 0 {
                    return Err("max-items must be positive".into());
                }
            }
            value if !value.starts_with("--") => {
                out.limit = value
                    .parse::<usize>()
                    .ok()
                    .filter(|n| *n > 0)
                    .unwrap_or(DEFAULT_MAX_LINES)
            }
            _ => return Err("unknown snapshot option".into()),
        }
        i += 1;
    }
    out.limit = out.limit.min(MAX_LINES);
    Ok(out)
}

impl BrowserRuntime {
    pub(super) async fn snapshot_verb(
        &self,
        page: &PageChannel,
        args: &[String],
    ) -> Result<String, String> {
        let options = parse(args)?;
        let capture = self.shared.begin_snapshot(page.session_id())?;
        self.send(page, "Accessibility.enable", json!({})).await?;
        let mut tree = self
            .send(page, "Accessibility.getFullAXTree", json!({}))
            .await?;
        self.redact_value(&mut tree);
        let mut nodes = parse_full_ax_tree(&tree)?;
        let document = self
            .send(page, "DOM.getDocument", json!({"depth":-1,"pierce":true}))
            .await?;
        sensitive::redact(&mut nodes, &document)?;
        if let Some(scope) = &options.scope {
            nodes = query::scoped(&nodes, scope)?;
        }
        let snapshot = build_snapshot_filtered(&nodes, options.limit, &capture, options.actions);
        let state = self.shared.state();
        let result = if options.json {
            json!({"page":{"title":state.title.chars().take(200).collect::<String>(),"url":state.url.chars().take(2000).collect::<String>()},"scope":options.scope,"items":snapshot.items,"truncated":snapshot.truncated,"maxItems":options.limit}).to_string()
        } else {
            format!("page: {} — {}\n{}", state.title, state.url, snapshot.text)
        };
        self.shared.finish_snapshot(&capture, snapshot)?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
