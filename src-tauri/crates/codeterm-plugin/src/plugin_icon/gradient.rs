use std::collections::HashSet;

#[derive(Default)]
pub(super) struct References {
    ids: HashSet<String>,
    used: HashSet<String>,
}
impl References {
    pub fn observe(&mut self, key: &str, value: &str) -> Option<()> {
        if key == "id" && !self.ids.insert(value.to_owned()) {
            return None;
        }
        if matches!(key, "fill" | "stroke") {
            if let Some(id) = local_reference(value) {
                self.used.insert(id.to_owned());
            }
        }
        Some(())
    }
    pub fn valid(&self) -> bool {
        self.used.is_subset(&self.ids)
    }
}

pub(super) fn valid_parent(name: &str, parent: Option<&str>) -> bool {
    match name {
        "svg" => parent.is_none(),
        "defs" => parent == Some("svg"),
        "linearGradient" | "radialGradient" => parent == Some("defs"),
        "stop" => matches!(parent, Some("linearGradient" | "radialGradient")),
        _ => !matches!(
            parent,
            None | Some("defs" | "linearGradient" | "radialGradient" | "stop")
        ),
    }
}

pub(super) fn definition(name: &str) -> bool {
    matches!(name, "defs" | "linearGradient" | "radialGradient" | "stop")
}
pub(super) fn attribute(name: &str, key: &str) -> bool {
    match name {
        "linearGradient" => matches!(
            key,
            "id" | "x1" | "x2" | "y1" | "y2" | "gradientUnits" | "gradientTransform"
        ),
        "radialGradient" => matches!(
            key,
            "id" | "cx" | "cy" | "r" | "fx" | "fy" | "gradientUnits" | "gradientTransform"
        ),
        "stop" => matches!(key, "offset" | "stop-color" | "stop-opacity"),
        _ => false,
    }
}
fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        && value
            .as_bytes()
            .first()
            .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
}
fn local_reference(value: &str) -> Option<&str> {
    value
        .strip_prefix("url(#")
        .and_then(|v| v.strip_suffix(')'))
        .filter(|v| valid_id(v))
}
fn color(value: &str) -> bool {
    value == "currentColor"
        || value.strip_prefix('#').is_some_and(|hex| {
            matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit())
        })
}
pub(super) fn paint(value: &str) -> bool {
    value == "none" || color(value) || local_reference(value).is_some()
}
pub(super) fn attribute_value(key: &str, value: &str) -> bool {
    match key {
        "id" => valid_id(value),
        "gradientUnits" => matches!(value, "userSpaceOnUse" | "objectBoundingBox"),
        "gradientTransform" => transform(value),
        "stop-color" => color(value),
        "stop-opacity" => bounded(value, 1.0),
        "offset" => value
            .strip_suffix('%')
            .map_or_else(|| bounded(value, 1.0), |v| bounded(v, 100.0)),
        _ => super::finite_number(value.strip_suffix('%').unwrap_or(value)),
    }
}
fn bounded(value: &str, max: f64) -> bool {
    value
        .parse::<f64>()
        .ok()
        .is_some_and(|v| v.is_finite() && v >= 0.0 && v <= max)
}
fn transform(mut value: &str) -> bool {
    if value.is_empty() || value.len() > 512 {
        return false;
    }
    while !value.trim().is_empty() {
        let Some((name, tail)) = value.trim().split_once('(') else {
            return false;
        };
        let Some((args, rest)) = tail.split_once(')') else {
            return false;
        };
        let args: Vec<_> = args
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|v| !v.is_empty())
            .collect();
        if !args.iter().all(|v| super::finite_number(v)) {
            return false;
        }
        let valid = match name.trim() {
            "matrix" => args.len() == 6,
            "translate" | "scale" => matches!(args.len(), 1 | 2),
            "rotate" => matches!(args.len(), 1 | 3),
            "skewX" | "skewY" => args.len() == 1,
            _ => false,
        };
        if !valid {
            return false;
        }
        value = rest;
    }
    true
}
