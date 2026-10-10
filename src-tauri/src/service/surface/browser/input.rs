use serde::Deserialize;
use serde_json::{json, Value};

/// Editing keys need the Windows virtual key code, or Chrome ignores them.
const VIRTUAL_KEYS: &[(&str, i64)] = &[
    ("Backspace", 8),
    ("Tab", 9),
    ("Enter", 13),
    ("Escape", 27),
    ("PageUp", 33),
    ("PageDown", 34),
    ("End", 35),
    ("Home", 36),
    ("ArrowLeft", 37),
    ("ArrowUp", 38),
    ("ArrowRight", 39),
    ("ArrowDown", 40),
    ("Delete", 46),
];

/// A viewer or agent input; the frontend sends precomputed CDP key fields, agents send flags.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InputArgs {
    pub kind: String,
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
    #[serde(default)]
    pub delta_x: Option<f64>,
    #[serde(default)]
    pub delta_y: Option<f64>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub windows_virtual_key_code: Option<i64>,
    #[serde(default)]
    pub modifiers: Option<i64>,
    #[serde(default)]
    pub button: Option<String>,
    #[serde(default)]
    pub click_count: Option<i64>,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub meta: bool,
}

impl InputArgs {
    pub(crate) fn click(x: f64, y: f64) -> Self {
        Self {
            kind: "click".into(),
            x: Some(x),
            y: Some(y),
            ..Default::default()
        }
    }

    pub(crate) fn key(key: &str, mods: &[String]) -> Self {
        let has = |name: &str| mods.iter().any(|m| m.eq_ignore_ascii_case(name));
        Self {
            kind: "key".into(),
            key: Some(key.to_string()),
            shift: has("shift"),
            ctrl: has("ctrl"),
            alt: has("alt"),
            meta: has("meta"),
            ..Default::default()
        }
    }

    fn modifier_bits(&self) -> i64 {
        self.modifiers.unwrap_or(
            (self.alt as i64)
                | ((self.ctrl as i64) << 1)
                | ((self.meta as i64) << 2)
                | ((self.shift as i64) << 3),
        )
    }

    fn point(&self) -> Result<(f64, f64), String> {
        match (self.x, self.y) {
            (Some(x), Some(y)) if x.is_finite() && y.is_finite() => Ok((x, y)),
            _ => Err(format!("{} input needs finite x and y", self.kind)),
        }
    }
}

pub(crate) fn virtual_key_code(key: &str) -> i64 {
    if let Some(named) = VIRTUAL_KEYS.iter().find(|(name, _)| *name == key) {
        return named.1;
    }
    match single_alphanumeric(key) {
        Some(c) => c.to_ascii_uppercase() as i64,
        None => 0,
    }
}

/// The DOM `code` for a key: `KeyA` / `Digit1` for letters and digits, the key name otherwise.
pub(crate) fn key_code(key: &str) -> String {
    match single_alphanumeric(key) {
        Some(c) if c.is_ascii_digit() => format!("Digit{c}"),
        Some(c) => format!("Key{}", c.to_ascii_uppercase()),
        None => key.to_string(),
    }
}

fn single_alphanumeric(key: &str) -> Option<char> {
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphanumeric() => Some(c),
        _ => None,
    }
}

/// The ordered CDP commands one input becomes.
pub(crate) fn input_commands(input: &InputArgs) -> Result<Vec<(&'static str, Value)>, String> {
    match input.kind.as_str() {
        "click" => {
            let (x, y) = input.point()?;
            let base = json!({
                "x": x,
                "y": y,
                "button": input.button.as_deref().unwrap_or("left"),
                "clickCount": input.click_count.unwrap_or(1),
            });
            Ok(["mouseMoved", "mousePressed", "mouseReleased"]
                .into_iter()
                .map(|kind| ("Input.dispatchMouseEvent", with_type(&base, kind)))
                .collect())
        }
        "wheel" => {
            let (x, y) = input.point()?;
            Ok(vec![(
                "Input.dispatchMouseEvent",
                json!({
                    "type": "mouseWheel",
                    "x": x,
                    "y": y,
                    "deltaX": input.delta_x.unwrap_or(0.0),
                    "deltaY": input.delta_y.unwrap_or(0.0),
                }),
            )])
        }
        "text" => {
            let text = input.text.as_deref().ok_or("text input needs text")?;
            Ok(vec![("Input.insertText", json!({ "text": text }))])
        }
        "key" => {
            let key = input
                .key
                .as_deref()
                .filter(|key| !key.is_empty())
                .ok_or("key input needs a key")?;
            let code = input
                .windows_virtual_key_code
                .unwrap_or_else(|| virtual_key_code(key));
            let common = json!({
                "key": key,
                "code": input.code.clone().unwrap_or_else(|| key_code(key)),
                "windowsVirtualKeyCode": code,
                "nativeVirtualKeyCode": code,
                "modifiers": input.modifier_bits(),
            });
            let text = input
                .text
                .clone()
                .or_else(|| (key == "Enter").then(|| "\r".to_string()));
            let down = match text {
                Some(text) => {
                    let mut down = with_type(&common, "keyDown");
                    down["text"] = Value::String(text);
                    down
                }
                None => with_type(&common, "rawKeyDown"),
            };
            Ok(vec![
                ("Input.dispatchKeyEvent", down),
                ("Input.dispatchKeyEvent", with_type(&common, "keyUp")),
            ])
        }
        other => Err(format!("unknown input kind \"{other}\"")),
    }
}

fn with_type(base: &Value, kind: &str) -> Value {
    let mut value = base.clone();
    value["type"] = Value::String(kind.to_string());
    value
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NavAction {
    Goto,
    Back,
    Forward,
    Reload,
}

impl NavAction {
    pub(crate) fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "goto" => Ok(Self::Goto),
            "back" => Ok(Self::Back),
            "forward" => Ok(Self::Forward),
            "reload" => Ok(Self::Reload),
            other => Err(format!("unknown nav action \"{other}\"")),
        }
    }
}

/// A scheme URL as is, a host-like word as a web URL, anything else as a search.
pub(crate) fn goto_href(target: &str) -> Result<String, String> {
    let target = target.trim();
    if target.is_empty() {
        return Err("usage: goto <url>".into());
    }
    if has_scheme(target) {
        return Ok(target.to_string());
    }
    let lower = target.to_ascii_lowercase();
    if lower.starts_with("about:") || lower.starts_with("data:") {
        return Ok(target.to_string());
    }
    if lower.starts_with("localhost") || lower.starts_with("127.0.0.1") {
        return Ok(format!("http://{target}"));
    }
    if target.contains('.') && !target.contains(' ') {
        return Ok(format!("https://{target}"));
    }
    let query: String = url::form_urlencoded::byte_serialize(target.as_bytes()).collect();
    Ok(format!("https://www.google.com/search?q={query}"))
}

fn has_scheme(target: &str) -> bool {
    target.split_once("://").is_some_and(|(scheme, _)| {
        !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphabetic())
    })
}

/// `Page.getNavigationHistory` → the entry id one step `back` or forward, if any.
pub(crate) fn history_entry(history: &Value, back: bool) -> Option<i64> {
    let current = history.get("currentIndex")?.as_i64()?;
    let next = if back { current - 1 } else { current + 1 };
    let index = usize::try_from(next).ok()?;
    history
        .get("entries")?
        .as_array()?
        .get(index)?
        .get("id")?
        .as_i64()
}

/// The centre of the first `DOM.getContentQuads` quad.
pub(crate) fn quad_center(result: &Value) -> Option<(f64, f64)> {
    let quad = result.get("quads")?.as_array()?.first()?.as_array()?;
    let points: Vec<f64> = quad.iter().filter_map(Value::as_f64).collect();
    if points.len() < 8 {
        return None;
    }
    let x = (points[0] + points[2] + points[4] + points[6]) / 4.0;
    let y = (points[1] + points[3] + points[5] + points[7]) / 4.0;
    Some((x, y))
}

#[cfg(test)]
mod tests;
