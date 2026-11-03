use super::BrowserRuntime;
use codeterm_browser::PageChannel;
use serde_json::json;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Options {
    limit: usize,
    selector: Option<String>,
    grep: Option<String>,
}

pub(super) fn parse(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        limit: 4000,
        selector: None,
        grep: None,
    };
    let mut limit_seen = false;
    let mut iter = args.iter();
    while let Some(word) = iter.next() {
        match word.as_str() {
            "--selector" | "--grep" => {
                let value = iter
                    .next()
                    .filter(|v| !v.is_empty())
                    .ok_or("text option requires a nonempty value")?;
                let slot = if word == "--selector" {
                    &mut options.selector
                } else {
                    &mut options.grep
                };
                if slot.replace(value.clone()).is_some() {
                    return Err("duplicate text option".into());
                }
            }
            _ if !limit_seen => {
                options.limit = word
                    .parse::<usize>()
                    .ok()
                    .filter(|n| (1..=200000).contains(n))
                    .ok_or("usage: text [1..200000] [--selector CSS] [--grep TEXT]")?;
                limit_seen = true;
            }
            _ => return Err("unknown text option".into()),
        }
    }
    Ok(options)
}

impl BrowserRuntime {
    pub(super) async fn text_verb(
        &self,
        page: &PageChannel,
        args: &[String],
    ) -> Result<super::output::ReadOutput, String> {
        let options = parse(args)?;
        let expression = format!(
            "({})({}, {}, {})",
            include_str!("text.js"),
            json!(options.selector),
            json!(options.grep),
            options.limit
        );
        let value = self.evaluate(page, &expression, false).await?;
        let text = value["text"].as_str().ok_or("text extraction returned no result")?;
        Ok(super::output::ReadOutput {
            text: text.to_string(),
            items: value["items"].as_u64().ok_or("text extraction returned no item count")? as usize,
            truncated: value["truncated"].as_bool().ok_or("text extraction returned no truncation state")?,
        })
    }
}

#[cfg(test)]
mod tests;
