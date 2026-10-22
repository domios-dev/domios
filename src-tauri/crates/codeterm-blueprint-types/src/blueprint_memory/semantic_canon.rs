use crate::blueprint_document::Document;
use regex::Regex;
use serde_json::{Map, Value};
use std::sync::OnceLock;

pub const NODE_WORDS: usize = 24;

pub fn semantic_canon(document: &Document, intent: &str) -> String {
    let gists = Gists { document };
    let mut head = Vec::new();
    for text in [intent, document.note.as_deref().unwrap_or_default()] {
        let text = plain(text);
        if !text.is_empty() {
            head.push(text);
        }
    }
    if let Some(trigger) = gists.trigger() {
        head.push(format!("trigger: {trigger}"));
    }
    let nodes = gists.nodes();
    let mut text = head.join(". ");
    if !nodes.is_empty() {
        if !text.is_empty() {
            text.push_str(". ");
        }
        text.push_str(&nodes.join("; "));
    }
    let text = collapse(&text.to_lowercase());
    if text.is_empty() {
        text
    } else {
        format!("{text}.")
    }
}

pub fn canonical_json(document: &Document) -> String {
    serde_json::to_value(document)
        .map(|value| sorted(&value).to_string())
        .unwrap_or_default()
}

fn sorted(value: &Value) -> Value {
    match value {
        Value::Object(fields) => {
            let mut keys = fields.keys().collect::<Vec<_>>();
            keys.sort();
            Value::Object(
                keys.into_iter()
                    .map(|key| (key.clone(), sorted(&fields[key])))
                    .collect::<Map<_, _>>(),
            )
        }
        Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

struct Gists<'a> {
    document: &'a Document,
}

impl Gists<'_> {
    fn trigger(&self) -> Option<String> {
        if let Some(pipeline) = &self.document.pipeline {
            let trigger = &pipeline["trigger"];
            let kind = trigger["type"].as_str()?;
            let detail = match kind {
                "schedule" => trigger
                    .get("every")
                    .map(|every| format!("every {}", self.value(every, duration))),
                "event" => trigger["kind"].as_str().map(words),
                "input" => trigger["kind"]
                    .as_str()
                    .map(|kind| format!("{} input", words(kind))),
                _ => None,
            };
            return Some(join(&[words(kind), detail.unwrap_or_default()]));
        }
        let watch = self.document.watch.as_ref()?;
        watch
            .get("every")
            .map(|every| format!("checks every {}", self.value(every, duration)))
    }

    fn nodes(&self) -> Vec<String> {
        if let Some(pipeline) = &self.document.pipeline {
            return pipeline["steps"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|step| self.step(step))
                .collect();
        }
        let Some(watch) = &self.document.watch else {
            return Vec::new();
        };
        let mut nodes = vec![self.watch_source(watch), self.watch_decision(watch)];
        if let Some(handlers) = watch["handlers"].as_object() {
            for (_, chain) in sorted_entries(handlers) {
                for handler in chain.as_array().into_iter().flatten() {
                    nodes.push(self.handler(handler));
                }
            }
        }
        if let Some(routes) = watch["routes"].as_object() {
            for (_, route) in sorted_entries(routes) {
                if let Some(gist) = route.as_str().and_then(route_gist) {
                    nodes.push(gist);
                }
            }
        }
        nodes
    }

    fn step(&self, step: &Value) -> String {
        let id = step["name"].as_str().map(words).unwrap_or_default();
        let with = step["with"].as_object();
        let slot = |name: &str| with.and_then(|with| with.get(name));
        let mut parts = does(step);
        if let Some(run) = step.get("run") {
            parts.push(self.command(run));
        } else {
            let kind = step["uses"].as_str().unwrap_or_default();
            parts.push(match kind {
                "http" => format!(
                    "get {}",
                    slot("url").map_or("{url}".into(), |url| self.value(url, url_gist))
                ),
                "github" => {
                    let resource = slot("resource").map(|value| self.value(value, words));
                    let rest = with
                        .into_iter()
                        .flat_map(|with| with.keys())
                        .filter(|key| !matches!(key.as_str(), "resource" | "timeout_ms"))
                        .map(|key| format!("{{{key}}}"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    join(&["github".into(), resource.unwrap_or_default(), rest])
                }
                "filter" => slot("pattern")
                    .map(|pattern| format!("matches {}", self.value(pattern, pattern_gist)))
                    .unwrap_or_else(|| "filter".into()),
                "ask" => slot("question")
                    .map(|question| format!("asks {}", self.value(question, words)))
                    .unwrap_or_else(|| "asks".into()),
                "convert" => join(&[
                    "converts".into(),
                    slot("from")
                        .map(|v| self.value(v, words))
                        .unwrap_or_default(),
                    slot("to")
                        .map(|v| format!("into {}", self.value(v, words)))
                        .unwrap_or_default(),
                ]),
                "agent" => join(&[
                    "agent task".into(),
                    slot("task")
                        .map(|v| self.value(v, words))
                        .unwrap_or_default(),
                ]),
                other => join(&[
                    words(other),
                    slot("text")
                        .map(|v| self.value(v, words))
                        .unwrap_or_default(),
                ]),
            });
        }
        node(&id, parts)
    }

    fn watch_source(&self, watch: &Value) -> String {
        let mut parts = Vec::new();
        if let Some(probe) = watch.get("probe") {
            parts.push(self.command(probe));
        } else if let Some(target) = watch.get("observe") {
            parts.push(format!("observes {}", self.target(target)));
        } else if let Some(input) = watch.get("input") {
            let kind = input["kind"].as_str().map(words).unwrap_or_default();
            parts.push(join(&["reads".into(), kind, "input".into()]));
        }
        if let Some(pattern) = watch["terminal"].get("pattern") {
            parts.push(format!("terminal {}", self.value(pattern, pattern_gist)));
        }
        node("source", parts)
    }

    fn watch_decision(&self, watch: &Value) -> String {
        let mut parts = Vec::new();
        if let Some(question) = watch.get("question") {
            parts.push(format!("asks {}", self.value(question, words)));
        }
        if let Some(matches) = watch["matches"].as_object() {
            for (_, pattern) in sorted_entries(matches) {
                parts.push(format!("matches {}", self.value(pattern, pattern_gist)));
            }
        }
        if watch["model_classification"].as_bool() == Some(true) {
            parts.push("by model".into());
        }
        node("decide", parts)
    }

    fn handler(&self, handler: &Value) -> String {
        let kind = handler["type"].as_str().unwrap_or_default();
        let id = handler["id"].as_str().map_or_else(|| words(kind), words);
        let mut parts = does(handler);
        parts.push(words(kind));
        if let Some(parameters) = handler["parameters"].as_object() {
            for (key, value) in sorted_entries(parameters) {
                let gist = match key {
                    "command" | "run" | "probe" => self.command(value),
                    "url" => self.value(value, url_gist),
                    "target" | "to" => self.target(value),
                    _ => self.value(value, words),
                };
                parts.push(gist);
            }
        }
        node(&id, parts)
    }

    fn command(&self, value: &Value) -> String {
        match input_reference(value) {
            Some(name) => format!("runs {}", self.describe_input(name)),
            None => self.value(value, command_gist),
        }
    }

    fn target(&self, value: &Value) -> String {
        match input_reference(value) {
            Some(name) => self.describe_input(name),
            None => value
                .as_str()
                .map_or_else(String::new, |text| match text.split_once(':') {
                    Some((kind, _)) => words(kind),
                    None => words(text),
                }),
        }
    }

    fn value(&self, value: &Value, literal: fn(&str) -> String) -> String {
        if let Some(name) = input_reference(value) {
            return self.describe_input(name);
        }
        match value {
            Value::String(text) => literal(text),
            Value::Number(_) => "{n}".into(),
            Value::Array(items) => items
                .iter()
                .map(|item| self.value(item, literal))
                .collect::<Vec<_>>()
                .join(" "),
            _ => String::new(),
        }
    }

    fn describe_input(&self, name: &str) -> String {
        self.document
            .inputs
            .get(name)
            .and_then(|input| input.description.as_deref())
            .map(|description| plain(description).to_lowercase())
            .filter(|description| !description.is_empty())
            .unwrap_or_else(|| format!("{{{name}}}"))
    }
}

fn does(node: &Value) -> Vec<String> {
    node["does"]
        .as_str()
        .map(plain)
        .into_iter()
        .filter(|text| !text.is_empty())
        .collect()
}

fn node(id: &str, parts: Vec<String>) -> String {
    let parts = parts
        .into_iter()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>();
    let text = if parts.is_empty() {
        id.to_owned()
    } else {
        format!("{id}: {}", parts.join(", "))
    };
    text.split_whitespace()
        .take(NODE_WORDS)
        .collect::<Vec<_>>()
        .join(" ")
}

fn route_gist(route: &str) -> Option<String> {
    let (kind, _) = route.split_once(':').unwrap_or((route, ""));
    match kind {
        "none" | "stop" | "" => None,
        kind => Some(node(&words(kind), Vec::new())),
    }
}

fn sorted_entries(map: &Map<String, Value>) -> Vec<(&str, &Value)> {
    let mut entries = map
        .iter()
        .map(|(key, value)| (key.as_str(), value))
        .collect::<Vec<_>>();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    entries
}

fn input_reference(value: &Value) -> Option<&str> {
    static WHOLE: OnceLock<Regex> = OnceLock::new();
    let text = value.as_str()?;
    WHOLE
        .get_or_init(|| {
            Regex::new(r"^\{\{\s*([A-Za-z_][A-Za-z0-9_]*)\s*\}\}$").expect("static regex")
        })
        .captures(text)
        .and_then(|captures| captures.get(1))
        .map(|name| name.as_str())
}

fn template(text: &str) -> String {
    static TEMPLATE: OnceLock<Regex> = OnceLock::new();
    TEMPLATE
        .get_or_init(|| Regex::new(r"\{\{\s*([A-Za-z0-9_]+)\s*\}\}").expect("static regex"))
        .replace_all(text, " {$1} ")
        .into_owned()
}

fn words(text: &str) -> String {
    static TOKEN: OnceLock<Regex> = OnceLock::new();
    let text = scrub(&template(text)).join(" ");
    TOKEN
        .get_or_init(|| {
            Regex::new(r"\{[A-Za-z0-9_]+\}|[A-Za-z][A-Za-z0-9]*").expect("static regex")
        })
        .find_iter(&text)
        .map(|token| token.as_str())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

fn scrub(text: &str) -> Vec<&str> {
    let tokens = text.split_whitespace().collect::<Vec<_>>();
    let sensitive = |token: &str| {
        let key = token
            .trim_matches(|c: char| !c.is_ascii_alphanumeric())
            .to_ascii_lowercase()
            .replace(['-', '_'], "");
        matches!(
            key.as_str(),
            "password"
                | "passwd"
                | "token"
                | "secret"
                | "apikey"
                | "key"
                | "authorization"
                | "bearer"
                | "basic"
                | "auth"
                | "pat"
        )
    };
    tokens
        .iter()
        .enumerate()
        .filter(|(index, token)| {
            let previous = index.checked_sub(1).map(|index| tokens[index]);
            !super::contains_credentials(token)
                && !previous.is_some_and(|previous| {
                    sensitive(previous)
                        || super::contains_credentials(&format!("{previous} {token}"))
                })
        })
        .map(|(_, token)| *token)
        .collect()
}

fn shell_token(token: &str) -> String {
    let token = token.trim_matches(|c| c == '\'' || c == '"');
    if token.is_empty() || matches!(token, "|" | "&&" | "||" | ";" | ">" | "<" | "2>&1") {
        return String::new();
    }
    if token.starts_with('{')
        && token.ends_with('}')
        && !token[1..token.len() - 1].contains(['{', '}'])
    {
        return token.to_owned();
    }
    if let Some(flag) = token.strip_prefix('-') {
        return words(
            flag.trim_start_matches('-')
                .split('=')
                .next()
                .unwrap_or_default(),
        );
    }
    if token.contains("://") {
        return "{url}".into();
    }
    if token.contains('/') {
        return "{path}".into();
    }
    if token.chars().any(|c| c.is_ascii_digit()) {
        return "{n}".into();
    }
    words(token)
}

fn command_gist(command: &str) -> String {
    let command = template(command);
    let tokens = scrub(&command)
        .into_iter()
        .map(shell_token)
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    join(&["runs".into(), tokens.join(" ")])
}

fn url_gist(url: &str) -> String {
    let url = url.trim();
    match url.split_once("://") {
        Some((_, rest)) => {
            let path = rest
                .split_once('/')
                .map(|(_, path)| words(path))
                .unwrap_or_default();
            join(&["{host}".into(), path])
        }
        None => words(url),
    }
}

fn pattern_gist(pattern: &str) -> String {
    static SYNTAX: OnceLock<Regex> = OnceLock::new();
    let text = template(pattern);
    words(
        &SYNTAX
            .get_or_init(|| Regex::new(r"\(\?[a-z]+\)|\\[a-zA-Z]").expect("static regex"))
            .replace_all(&text, " "),
    )
}

fn duration(text: &str) -> String {
    static DURATION: OnceLock<Regex> = OnceLock::new();
    let text = text.trim().to_lowercase();
    if DURATION
        .get_or_init(|| Regex::new(r"^(\d+[dhms])+$").expect("static regex"))
        .is_match(&text)
    {
        text
    } else {
        words(&text)
    }
}

fn plain(text: &str) -> String {
    collapse(text).trim_end_matches('.').trim().to_owned()
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn join(parts: &[String]) -> String {
    collapse(&parts.join(" "))
}

#[cfg(test)]
mod tests;
