use std::collections::{HashMap, HashSet};

use regex::Regex;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::sync::LazyLock;

#[derive(Debug, Clone, Deserialize)]
struct ToolSchema {
    tools: Vec<ToolSpec>,
    #[serde(default)]
    aliases: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ToolSpec {
    name: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    optional: Vec<String>,
}

#[derive(Debug)]
struct SchemaIndex {
    tools: HashMap<String, ToolSpec>,
    aliases: HashMap<String, String>,
}

#[derive(Debug)]
struct Candidate {
    tool: String,
    args: Map<String, Value>,
    span: (usize, usize),
    confidence: f64,
}

#[derive(Debug)]
struct Segment<'a> {
    text: &'a str,
    offset: usize,
    confidence: f64,
}

static FENCE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)```[^\r\n`]*\r?\n(.*?)```").unwrap());
static TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<\|tool_call\|>(.*?)<\|/tool_call\|>").unwrap());
static CALL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)call:([A-Za-z_][A-Za-z0-9_.-]*(?::[A-Za-z_][A-Za-z0-9_.-]*)?)\s*\{").unwrap()
});
static UNQUOTED_KEY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)(^|[{\[,]\s*)([A-Za-z_][A-Za-z0-9_-]*)(\s*:)"#).unwrap());

pub(crate) fn parse_to_json(raw_text: &str, schema_json: &str) -> String {
    match parse(raw_text, schema_json) {
        Some(value) => value.to_string(),
        None => "null".to_string(),
    }
}

fn parse(raw_text: &str, schema_json: &str) -> Option<Value> {
    let schema = SchemaIndex::parse(schema_json)?;
    let mut best: Option<Candidate> = None;

    for segment in segments(raw_text) {
        for candidate in parse_segment(&segment, &schema) {
            if best
                .as_ref()
                .is_none_or(|current| candidate.confidence > current.confidence)
            {
                best = Some(candidate);
            }
        }
    }

    best.map(|candidate| {
        serde_json::json!({
            "tool": candidate.tool,
            "args": Value::Object(candidate.args),
            "confidence": candidate.confidence.clamp(0.0, 1.0),
            "span": [candidate.span.0, candidate.span.1],
        })
    })
}

impl SchemaIndex {
    fn parse(schema_json: &str) -> Option<Self> {
        let schema: ToolSchema = serde_json::from_str(schema_json).ok()?;
        let tools = schema
            .tools
            .into_iter()
            .filter(|tool| !tool.name.trim().is_empty())
            .map(|tool| (tool.name.clone(), tool))
            .collect::<HashMap<_, _>>();
        if tools.is_empty() {
            return None;
        }
        Some(Self {
            tools,
            aliases: schema.aliases,
        })
    }

    fn validate(
        &self,
        tool: &str,
        args: Map<String, Value>,
    ) -> Option<(String, Map<String, Value>)> {
        let spec = self.tools.get(tool)?;
        let allowed = spec
            .args
            .iter()
            .chain(spec.optional.iter())
            .cloned()
            .collect::<HashSet<_>>();
        let required = spec.args.iter().cloned().collect::<HashSet<_>>();
        let mut normalized = Map::new();

        for (key, value) in args {
            let normalized_key = self.aliases.get(&key).cloned().unwrap_or(key);
            if allowed.contains(&normalized_key) {
                normalized.insert(normalized_key, value);
            }
        }

        if required.iter().all(|key| normalized.contains_key(key)) {
            Some((spec.name.clone(), normalized))
        } else {
            None
        }
    }
}

fn segments(raw_text: &str) -> Vec<Segment<'_>> {
    let mut out = vec![Segment {
        text: raw_text,
        offset: 0,
        confidence: 0.0,
    }];

    for caps in FENCE_RE.captures_iter(raw_text) {
        if let Some(body) = caps.get(1) {
            out.push(Segment {
                text: body.as_str(),
                offset: body.start(),
                confidence: 0.04,
            });
        }
    }

    let bytes = raw_text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            if let Some(end_rel) = raw_text[i + 1..].find('`') {
                let start = i + 1;
                let end = start + end_rel;
                let body = &raw_text[start..end];
                if body.contains('{') && body.contains('}') {
                    out.push(Segment {
                        text: body,
                        offset: start,
                        confidence: 0.03,
                    });
                }
                i = end + 1;
                continue;
            }
        }
        i += 1;
    }

    out
}

fn parse_segment(segment: &Segment<'_>, schema: &SchemaIndex) -> Vec<Candidate> {
    let mut out = Vec::new();

    for caps in TAG_RE.captures_iter(segment.text) {
        if let Some(body) = caps.get(1) {
            let nested = Segment {
                text: body.as_str(),
                offset: segment.offset + body.start(),
                confidence: segment.confidence + 0.08,
            };
            out.extend(parse_segment(&nested, schema));
        }
    }

    for caps in CALL_RE.captures_iter(segment.text) {
        let Some(name_match) = caps.get(1) else {
            continue;
        };
        let Some(open_rel) = segment.text[caps.get(0).unwrap().start()..].find('{') else {
            continue;
        };
        let open = caps.get(0).unwrap().start() + open_rel;
        let Some((args_value, end, penalty)) = parse_json_at(segment.text, open) else {
            continue;
        };
        let Some(args) = value_object(args_value) else {
            continue;
        };
        let tool = name_match.as_str().rsplit(':').next().unwrap_or_default();
        if let Some((tool, args)) = schema.validate(tool, args) {
            out.push(Candidate {
                tool,
                args,
                span: (
                    segment.offset + caps.get(0).unwrap().start(),
                    segment.offset + end,
                ),
                confidence: 0.92 + segment.confidence - penalty,
            });
        }
    }

    for (value, span, penalty) in json_candidates(segment.text) {
        out.extend(candidate_from_value(
            value,
            (segment.offset + span.0, segment.offset + span.1),
            0.86 + segment.confidence - penalty,
            schema,
        ));
    }

    out
}

fn json_candidates(text: &str) -> Vec<(Value, (usize, usize), f64)> {
    let mut out = Vec::new();
    for (start, ch) in text.char_indices() {
        if ch != '{' {
            continue;
        }
        if !has_json_candidate_boundary(text, start) {
            continue;
        }
        if let Some((value, end, penalty)) = parse_json_at(text, start) {
            out.push((value, (start, end), penalty));
        }
    }
    out
}

fn has_json_candidate_boundary(text: &str, start: usize) -> bool {
    let prefix = &text[..start];
    if prefix.trim().is_empty() || prefix.ends_with('\n') || prefix.ends_with('\r') {
        return true;
    }
    prefix
        .chars()
        .rev()
        .find(|ch| !ch.is_whitespace())
        .is_none_or(|ch| matches!(ch, '`' | '>' | ':' | '[' | ',' | '='))
}

fn candidate_from_value(
    value: Value,
    span: (usize, usize),
    confidence: f64,
    schema: &SchemaIndex,
) -> Vec<Candidate> {
    let mut out = Vec::new();
    match value {
        Value::Object(mut object) => {
            if let Some(candidate) =
                openai_tool_calls_candidate(&object, span, confidence + 0.06, schema)
            {
                out.push(candidate);
            }
            if let Some(candidate) =
                dot_tool_call_candidate(&object, span, confidence + 0.04, schema)
            {
                out.push(candidate);
            }
            if let Some(candidate) = bare_tool_candidate(&mut object, span, confidence, schema) {
                out.push(candidate);
            }
        }
        Value::Array(items) => {
            for item in items {
                out.extend(candidate_from_value(item, span, confidence - 0.03, schema));
            }
        }
        _ => {}
    }
    out
}

fn openai_tool_calls_candidate(
    object: &Map<String, Value>,
    span: (usize, usize),
    confidence: f64,
    schema: &SchemaIndex,
) -> Option<Candidate> {
    let calls = object.get("tool_calls")?.as_array()?;
    let first = calls.first()?.as_object()?;
    let function = first.get("function")?.as_object()?;
    let tool = function.get("name")?.as_str()?;
    let args_value = function.get("arguments")?;
    let args = match args_value {
        Value::String(s) => parse_json_at(s, 0)
            .and_then(|(value, _, _)| value_object(value))
            .unwrap_or_default(),
        Value::Object(map) => map.clone(),
        _ => return None,
    };
    schema.validate(tool, args).map(|(tool, args)| Candidate {
        tool,
        args,
        span,
        confidence,
    })
}

fn dot_tool_call_candidate(
    object: &Map<String, Value>,
    span: (usize, usize),
    confidence: f64,
    schema: &SchemaIndex,
) -> Option<Candidate> {
    let calls = object
        .get(".tool_call")
        .or_else(|| object.get("tool_call"))?
        .as_array()?;
    let first = calls.first()?.as_object()?;
    let tool = first
        .get("name")
        .or_else(|| first.get("tool"))
        .and_then(Value::as_str)?;
    let args = first
        .get("args")
        .or_else(|| first.get("arguments"))
        .and_then(Value::as_object)?
        .clone();
    schema.validate(tool, args).map(|(tool, args)| Candidate {
        tool,
        args,
        span,
        confidence,
    })
}

fn bare_tool_candidate(
    object: &mut Map<String, Value>,
    span: (usize, usize),
    confidence: f64,
    schema: &SchemaIndex,
) -> Option<Candidate> {
    let tool = object
        .get("tool")
        .or_else(|| object.get("name"))
        .and_then(Value::as_str)?
        .to_string();
    let args = object
        .remove("args")
        .or_else(|| object.remove("arguments"))
        .and_then(value_object)?;
    schema.validate(&tool, args).map(|(tool, args)| Candidate {
        tool,
        args,
        span,
        confidence,
    })
}

fn value_object(value: Value) -> Option<Map<String, Value>> {
    match value {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

fn parse_json_at(text: &str, start: usize) -> Option<(Value, usize, f64)> {
    let (slice, end, mut penalty) = object_slice(text, start)?;
    match serde_json::from_str::<Value>(slice) {
        Ok(value) => return Some((value, end, penalty)),
        Err(_) => {}
    }

    let repaired = repair_json(slice)?;
    if repaired != slice {
        penalty += 0.12;
    }
    serde_json::from_str::<Value>(&repaired)
        .ok()
        .map(|value| (value, end, penalty))
}

fn object_slice(text: &str, start: usize) -> Option<(&str, usize, f64)> {
    let bytes = text.as_bytes();
    if bytes.get(start) != Some(&b'{') {
        return None;
    }

    let mut depth = 0usize;
    let mut in_string = false;
    let mut escape = false;
    let mut last = start;

    for (rel, ch) in text[start..].char_indices() {
        let idx = start + rel;
        last = idx + ch.len_utf8();
        if in_string {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some((&text[start..last], last, 0.0));
                }
            }
            _ => {}
        }
    }

    let tail = text[start..last].trim_end();
    let open = tail.as_bytes().iter().filter(|&&b| b == b'{').count();
    let close = tail.as_bytes().iter().filter(|&&b| b == b'}').count();
    if open == close + 1 {
        Some((&text[start..start + tail.len()], start + tail.len(), 0.18))
    } else {
        None
    }
}

fn repair_json(input: &str) -> Option<String> {
    let mut out = input
        .replace(['\u{201c}', '\u{201d}'], "\"")
        .replace(['\u{2018}', '\u{2019}'], "'");
    out = single_quoted_strings(&out);
    out = UNQUOTED_KEY_RE.replace_all(&out, "$1\"$2\"$3").to_string();

    let open = out.as_bytes().iter().filter(|&&b| b == b'{').count();
    let close = out.as_bytes().iter().filter(|&&b| b == b'}').count();
    if open == close + 1 {
        out.push('}');
    }

    Some(out)
}

fn single_quoted_strings(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\'' {
            out.push(ch);
            continue;
        }

        let mut content = String::new();
        let mut closed = false;
        while let Some(inner) = chars.next() {
            if inner == '\\' {
                if let Some(next) = chars.next() {
                    content.push(next);
                }
            } else if inner == '\'' {
                closed = true;
                break;
            } else {
                content.push(inner);
            }
        }

        if closed {
            out.push('"');
            out.push_str(&content.replace('"', "\\\""));
            out.push('"');
        } else {
            out.push('\'');
            out.push_str(&content);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMA: &str = r#"{
        "tools": [
            {"name":"exec","args":["cmd"],"optional":[]},
            {"name":"read_file","args":["path"],"optional":[]},
            {"name":"write_file","args":["path","content"],"optional":[]}
        ],
        "aliases": {"command":"cmd","file":"path","filepath":"path"}
    }"#;

    fn parsed(raw: &str) -> Option<Value> {
        parse(raw, SCHEMA)
    }

    fn assert_call(raw: &str, tool: &str, key: &str, value: &str) {
        let out = parsed(raw).unwrap_or_else(|| panic!("no call parsed from {raw:?}"));
        assert_eq!(out["tool"], tool);
        assert_eq!(out["args"][key], value);
        assert!(out["confidence"].as_f64().unwrap() > 0.5);
        assert_eq!(out["span"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn golden_positive_corpus() {
        let cases = [
            (
                "```codeterm-tool\n{\"tool\":\"exec\",\"args\":{\"cmd\":\"ls\"}}\n```",
                "exec",
                "cmd",
                "ls",
            ),
            (
                "```json\n{\"tool\":\"read_file\",\"args\":{\"path\":\"src/main.rs\"}}\n```",
                "read_file",
                "path",
                "src/main.rs",
            ),
            (
                "<|tool_call|>call:exec{\"cmd\":\"ls\"}<|/tool_call|>",
                "exec",
                "cmd",
                "ls",
            ),
            (
                r#"{"tool_calls":[{"function":{"name":"exec","arguments":"{\"cmd\":\"ls\"}"}}]}"#,
                "exec",
                "cmd",
                "ls",
            ),
            (
                "{\"tool\":\"exec\",\"args\":{\"cmd\":\"ls\"}}",
                "exec",
                "cmd",
                "ls",
            ),
            (
                "I will do it.\n{\"tool\":\"exec\",\"args\":{\"cmd\":\"ls\"}}\nDone.",
                "exec",
                "cmd",
                "ls",
            ),
            (
                "{\"tool\":\"exec\",\"args\":{\"command\":\"ls\"}}",
                "exec",
                "cmd",
                "ls",
            ),
            (
                "{\"tool\":\"exec\",\"args\":{\"cmd\":\"ls\"}",
                "exec",
                "cmd",
                "ls",
            ),
        ];

        for (raw, tool, key, value) in cases {
            assert_call(raw, tool, key, value);
        }
    }

    #[test]
    fn conservative_json_repair() {
        assert_call(
            "{tool:'write_file',args:{file:'a.txt',content:'hi'}} trailing",
            "write_file",
            "path",
            "a.txt",
        );
    }

    #[test]
    fn negative_corpus() {
        let cases = [
            "plain prose with no call",
            "```python\nprint({'tool': 'exec', 'args': {'cmd': 'ls'}})\n```",
            "{\"tool\":\"exec\"",
            "{\"tool\":\"rm_rf\",\"args\":{\"cmd\":\"/\"}}",
        ];

        for raw in cases {
            assert_eq!(parsed(raw), None, "unexpected call from {raw:?}");
        }
    }
}
