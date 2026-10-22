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
    let inputs = document
        .inputs
        .values()
        .filter_map(|input| input.description.as_deref().map(plain))
        .filter(|description| !description.is_empty())
        .collect::<Vec<_>>();
    if !inputs.is_empty() {
        head.push(format!("inputs: {}", inputs.join("; ")));
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
        use crate::blueprint_document::Trigger;
        match self.document.trigger.as_ref() {
            Some(Trigger::Schedule { every }) => Some(format!(
                "schedule every {}",
                self.value(&serde_json::to_value(every).ok()?, duration)
            )),
            Some(Trigger::Event { kind, .. }) => Some(format!(
                "event {}",
                self.value(&serde_json::to_value(kind).ok()?, words)
            )),
            Some(Trigger::Input { kind, .. }) => Some(format!(
                "{} input",
                self.value(&serde_json::to_value(kind).ok()?, words)
            )),
            None => self
                .document
                .steps
                .first()
                .and_then(|n| n.every.as_ref())
                .and_then(|v| serde_json::to_value(v).ok())
                .map(|v| format!("checks every {}", self.value(&v, duration))),
        }
    }
    fn nodes(&self) -> Vec<String> {
        self.document
            .nodes()
            .filter_map(|node| serde_json::to_value(node).ok())
            .map(|node| self.step(&node))
            .collect()
    }
    fn step(&self, step: &Value) -> String {
        let id = step["id"].as_str().map(words).unwrap_or_default();
        let with = step["with"].as_object();
        let slot = |name: &str| with.and_then(|with| with.get(name));
        let mut parts = does(step);
        if let Some(run) = step.get("run") {
            parts.push(self.command(run));
        } else if let Some(code) = step.get("code") {
            parts.push(format!(
                "{} code {}",
                code["lang"].as_str().unwrap_or_default(),
                code["src"].as_str().map(command_gist).unwrap_or_default()
            ));
        } else if let Some(call) = step.get("call") {
            parts.push(format!(
                "calls {}",
                call["ref"].as_str().map(words).unwrap_or_default()
            ));
        } else {
            let kind = step["uses"].as_str().unwrap_or_default();
            parts.push(match kind {
                "probe" | "command" => slot("command")
                    .map(|v| self.command(v))
                    .unwrap_or_else(|| "runs command".into()),
                "observe" | "terminal" => join(&[
                    "observes".into(),
                    slot("kind")
                        .map(|v| self.value(v, words))
                        .unwrap_or_default(),
                ]),
                "input" => join(&[
                    slot("kind")
                        .map(|v| self.value(v, words))
                        .unwrap_or_default(),
                    "input".into(),
                    slot("convert_to")
                        .map(|v| format!("converts into {}", self.value(v, words)))
                        .unwrap_or_default(),
                ]),
                "send" => join(&[
                    "send".into(),
                    slot("target")
                        .map(|v| {
                            self.value(v, |v| v.split(':').next().map(words).unwrap_or_default())
                        })
                        .unwrap_or_default(),
                    slot("text")
                        .map(|v| self.value(v, words))
                        .unwrap_or_default(),
                ]),
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
        for pattern in step["matches"]
            .as_object()
            .into_iter()
            .flat_map(|m| m.values())
        {
            parts.push(format!("matches {}", self.value(pattern, pattern_gist)));
        }
        node(&id, parts)
    }

    fn command(&self, value: &Value) -> String {
        match input_reference(value) {
            Some(name) => format!("runs {}", self.describe_input(name)),
            None => self.value(value, command_gist),
        }
    }

    fn value(&self, value: &Value, literal: fn(&str) -> String) -> String {
        if let Some(name) = input_reference(value) {
            return self.describe_input(name);
        }
        let value = value.get("lit").unwrap_or(value);
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

fn input_reference(value: &Value) -> Option<&str> {
    value.get("input").and_then(Value::as_str)
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
    let text = scrub(&template(text), false).join(" ");
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

fn scrub(text: &str, keywords: bool) -> Vec<&str> {
    scrub_tokens(text.split_whitespace().collect(), keywords)
}

fn scrub_tokens(tokens: Vec<&str>, keywords: bool) -> Vec<&str> {
    let sensitive = |token: &str| {
        let key = token
            .split([':', '='])
            .next()
            .unwrap_or_default()
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
    let mut kept = Vec::new();
    let mut skip_next = false;
    for (index, token) in tokens.iter().enumerate() {
        let next = tokens.get(index + 1).copied().unwrap_or_default();
        let bearing = (keywords && sensitive(token))
            || super::contains_credentials(token)
            || (!keywords && super::contains_credentials(&format!("{token} {next}")));
        if skip_next {
            skip_next = false;
            continue;
        }
        if bearing {
            skip_next = !keywords
                || (sensitive(token)
                    && !token.contains([':', '='])
                    && !token.contains(char::is_whitespace));
            continue;
        }
        kept.push(*token);
    }
    kept
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
    let Ok(arguments) = shell_words::split(&command) else {
        return String::new();
    };
    let tokens = scrub_tokens(arguments.iter().map(String::as_str).collect(), true)
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
