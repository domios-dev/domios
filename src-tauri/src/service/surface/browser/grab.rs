use serde::Deserialize;
use serde_json::{json, Value};

/// Runs on the picked node via `Runtime.callFunctionOn`; returns the element facts as JSON text.
pub(crate) const COLLECT_IN_PAGE: &str = r##"function () {
  const el = this.nodeType === 1 ? this : this.parentElement;
  const esc = (s) => (window.CSS && CSS.escape ? CSS.escape(s) : s);
  const parts = [];
  for (let e = el; e && e.nodeType === 1 && parts.length < 6; e = e.parentElement) {
    if (e.id) { parts.unshift("#" + esc(e.id)); break; }
    let part = e.tagName.toLowerCase();
    const classes = [...e.classList].filter((c) => !/^\d/.test(c)).slice(0, 2);
    if (classes.length) part += "." + classes.map(esc).join(".");
    const parent = e.parentElement;
    if (parent) {
      const same = [...parent.children].filter((c) => c.tagName === e.tagName);
      if (same.length > 1) part += ":nth-of-type(" + (same.indexOf(e) + 1) + ")";
    }
    parts.unshift(part);
  }
  const rect = el.getBoundingClientRect();
  const cs = getComputedStyle(el);
  const styles = ["color", "backgroundColor", "fontSize", "fontWeight", "fontFamily", "padding", "margin", "borderRadius", "border", "display", "width", "height", "opacity"].map((k) => [k, String(cs[k])]);
  let react = null, source = null;
  const fiberKey = Object.keys(el).find((k) => k.startsWith("__reactFiber$"));
  if (fiberKey) {
    const names = [];
    for (let f = el[fiberKey]; f && names.length < 3; f = f.return) {
      if (typeof f.type === "function" && (f.type.displayName || f.type.name)) names.push(f.type.displayName || f.type.name);
      if (!source && f._debugSource) source = f._debugSource.fileName + ":" + f._debugSource.lineNumber;
    }
    react = names.join(" < ") || null;
  }
  const attributes = ["id", "name", "type", "href", "role", "aria-label", "placeholder", "title", "data-testid", "disabled"]
    .filter((name) => el.hasAttribute(name)).map((name) => [name, String(el.getAttribute(name))]);
  const textOf = (n) => (n.innerText || n.textContent || "").replace(/\s+/g, " ").trim();
  const nearbyText = [...(el.parentElement ? el.parentElement.children : [])].filter((s) => s !== el).map(textOf).filter(Boolean).slice(0, 4).map((t) => t.slice(0, 80));
  const ancestorPath = [];
  for (let e = el.parentElement; e && e !== document.body && ancestorPath.length < 6; e = e.parentElement) ancestorPath.unshift(e.tagName.toLowerCase() + (e.id ? "#" + e.id : ""));
  return JSON.stringify({
    tagName: el.tagName.toLowerCase(), selector: parts.join(" > "), text: textOf(el).slice(0, 200),
    classes: typeof el.className === "string" ? el.className : "", attributes, styles, react, source, nearbyText, ancestorPath,
    rectViewport: { x: rect.x, y: rect.y, width: rect.width, height: rect.height },
    rectPage: { x: rect.x + scrollX, y: rect.y + scrollY, width: rect.width, height: rect.height },
    page: { url: location.href, title: document.title, viewport: innerWidth + "x" + innerHeight },
  });
}"##;

pub(crate) fn highlight_config() -> Value {
    json!({
        "showInfo": true,
        "showAccessibilityInfo": true,
        "contentColor": { "r": 99, "g": 102, "b": 241, "a": 0.22 },
        "borderColor": { "r": 99, "g": 102, "b": 241, "a": 0.9 },
    })
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub(crate) struct PageFacts {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub viewport: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub(crate) struct Accessibility {
    pub role: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GrabTarget {
    pub tag_name: String,
    #[serde(default)]
    pub selector: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub classes: String,
    #[serde(default)]
    pub attributes: Vec<(String, String)>,
    #[serde(default)]
    pub styles: Vec<(String, String)>,
    #[serde(default)]
    pub react: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub nearby_text: Vec<String>,
    #[serde(default)]
    pub ancestor_path: Vec<String>,
    #[serde(default)]
    pub rect_page: Rect,
    #[serde(default)]
    pub page: PageFacts,
    #[serde(skip)]
    pub accessibility: Accessibility,
}

impl GrabTarget {
    /// The frontend card shape: the collected facts plus the accessibility role and name.
    pub(crate) fn to_json(&self) -> Value {
        let pairs = |items: &[(String, String)]| -> Value {
            items
                .iter()
                .map(|(key, value)| (key.clone(), Value::String(value.clone())))
                .collect::<serde_json::Map<String, Value>>()
                .into()
        };
        json!({
            "tagName": self.tag_name,
            "selector": self.selector,
            "text": self.text,
            "classes": self.classes,
            "attributes": pairs(&self.attributes),
            "styles": pairs(&self.styles),
            "react": self.react,
            "source": self.source,
            "nearbyText": self.nearby_text,
            "ancestorPath": self.ancestor_path,
            "rectPage": { "x": self.rect_page.x, "y": self.rect_page.y, "width": self.rect_page.width, "height": self.rect_page.height },
            "page": { "url": self.page.url, "title": self.page.title, "viewport": self.page.viewport },
            "accessibility": { "role": self.accessibility.role, "name": self.accessibility.name },
        })
    }
}

/// `Accessibility.getPartialAXTree` → the first non-ignored node's role and name.
pub(crate) fn accessibility_of(tree: &Value) -> Accessibility {
    let nodes = tree.get("nodes").and_then(Value::as_array);
    let node = nodes.and_then(|nodes| {
        nodes
            .iter()
            .find(|node| {
                !node
                    .get("ignored")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            })
            .or_else(|| nodes.first())
    });
    let text = |field: &str| {
        node.and_then(|node| node.get(field))
            .and_then(|value| value.get("value"))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    Accessibility {
        role: text("role"),
        name: text("name"),
    }
}

pub(crate) fn element_label(target: &GrabTarget) -> String {
    let a11y = &target.accessibility;
    let base = match (&a11y.role, &a11y.name) {
        (Some(role), Some(name)) if !role.is_empty() && !name.is_empty() => {
            format!("{role} \"{name}\"")
        }
        _ if !target.text.is_empty() => format!(
            "{} \"{}\"",
            target.tag_name,
            target.text.chars().take(60).collect::<String>()
        ),
        _ => target.tag_name.clone(),
    };
    match target
        .react
        .as_deref()
        .and_then(|react| react.split(" < ").next())
    {
        Some(component) if !component.is_empty() => format!("{component} {base}"),
        _ => base,
    }
}

pub(crate) const INTENTS: &[&str] = &["question", "change", "bug"];

pub(crate) fn parse_intent(raw: Option<&str>) -> Result<&'static str, String> {
    let raw = raw.unwrap_or("question");
    INTENTS
        .iter()
        .find(|intent| **intent == raw)
        .copied()
        .ok_or_else(|| format!("intent must be one of {}", INTENTS.join(", ")))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AnnotateRequest {
    pub locator: Vec<String>,
    pub comment: String,
    pub intent: &'static str,
}

pub(crate) const ANNOTATE_USAGE: &str =
    "usage: annotate @eN|<role> <name> <comment> [--intent question|change|bug] [--of <agent>]";

/// `annotate <@eN | role name> <comment> [--intent I]`: the locator follows `click`'s rules.
pub(crate) fn parse_annotate(args: &[String]) -> Result<AnnotateRequest, String> {
    let mut words = Vec::new();
    let mut intent = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if arg == "--intent" {
            intent = Some(rest.next().ok_or(ANNOTATE_USAGE)?.as_str());
        } else {
            words.push(arg.clone());
        }
    }
    let intent = parse_intent(intent)?;
    let (comment, locator) = words.split_last().ok_or(ANNOTATE_USAGE)?;
    if locator.is_empty() || comment.trim().is_empty() {
        return Err(ANNOTATE_USAGE.into());
    }
    Ok(AnnotateRequest {
        locator: locator.to_vec(),
        comment: comment.clone(),
        intent,
    })
}

pub(crate) struct Annotation<'a> {
    pub target: &'a GrabTarget,
    pub intent: &'a str,
    pub comment: &'a str,
    pub screenshot_path: Option<&'a str>,
    /// The annotating agent; `None` is the user.
    pub sender: Option<&'a str>,
}

/// The markdown the owner agent receives, in the shape of the POC's `annotationMarkdown`.
pub(crate) fn annotation_markdown(annotation: &Annotation<'_>) -> String {
    let t = annotation.target;
    let comment = if annotation.comment.trim().is_empty() {
        "(no comment)"
    } else {
        annotation.comment
    };
    let (from, speaker) = match annotation.sender {
        Some(agent) => (agent, agent),
        None => ("the user", "User"),
    };
    let mut lines = vec![
        format!("## Browser annotation from {from}"),
        format!(
            "**Page:** {} — `{}` (viewport {})",
            t.page.title, t.page.url, t.page.viewport
        ),
        String::new(),
        format!("### {}", element_label(t)),
        format!("**Intent:** {}", annotation.intent),
        format!("**{speaker} says:** {comment}"),
        format!("**Selector:** `{}`", t.selector),
    ];
    if let Some(role) = t
        .accessibility
        .role
        .as_deref()
        .filter(|role| !role.is_empty())
    {
        lines.push(format!(
            "**Accessibility:** role `{role}`, name \"{}\"",
            t.accessibility.name.as_deref().unwrap_or("")
        ));
    }
    if let Some(react) = t.react.as_deref().filter(|react| !react.is_empty()) {
        lines.push(format!("**React:** {react}"));
    }
    if let Some(source) = t.source.as_deref().filter(|source| !source.is_empty()) {
        lines.push(format!("**Source:** `{source}`"));
    }
    if !t.text.is_empty() {
        lines.push(format!("**Text:** \"{}\"", t.text));
    }
    if !t.attributes.is_empty() {
        let attributes: Vec<String> = t
            .attributes
            .iter()
            .map(|(key, value)| format!("{key}=\"{value}\""))
            .collect();
        lines.push(format!("**Attributes:** {}", attributes.join(" ")));
    }
    if !t.classes.is_empty() {
        let classes: String = t.classes.chars().take(200).collect();
        lines.push(format!("**Classes:** `{classes}`"));
    }
    let r = &t.rect_page;
    lines.push(format!(
        "**Box:** {},{} {}×{}",
        r.x.round(),
        r.y.round(),
        r.width.round(),
        r.height.round()
    ));
    let styles: Vec<String> = t
        .styles
        .iter()
        .map(|(key, value)| format!("{key}: {value}"))
        .collect();
    lines.push(format!("**Computed styles:** {}", styles.join("; ")));
    if !t.nearby_text.is_empty() {
        let nearby: Vec<String> = t.nearby_text.iter().map(|s| format!("\"{s}\"")).collect();
        lines.push(format!("**Nearby text:** {}", nearby.join(", ")));
    }
    if !t.ancestor_path.is_empty() {
        lines.push(format!("**Inside:** {}", t.ancestor_path.join(" > ")));
    }
    if let Some(path) = annotation.screenshot_path {
        lines.push(format!("**Element screenshot:** `{path}`"));
    }
    lines.join("\n")
}

/// The clip for the element screenshot, capped like the POC; `None` for a zero-size box.
pub(crate) fn screenshot_clip(rect: &Rect) -> Option<Value> {
    (rect.width > 0.0 && rect.height > 0.0).then(|| {
        json!({
            "x": rect.x,
            "y": rect.y,
            "width": rect.width.min(1600.0),
            "height": rect.height.min(1200.0),
            "scale": 1,
        })
    })
}

#[cfg(test)]
mod tests;
