use std::io::Read;
use std::path::{Component, Path};

use codeterm_types::svg::{sanitize_svg, SvgUse};

pub const MAX_PLUGIN_ICON_BYTES: usize = codeterm_types::svg::MAX_SVG_BYTES;

pub mod registry;

pub fn load_plugin_icon(bundle_dir: &Path, relative_path: &str) -> Option<String> {
    if !valid_relative_svg_path(relative_path) {
        return None;
    }
    let root = std::fs::canonicalize(bundle_dir).ok()?;
    let asset = std::fs::canonicalize(root.join(relative_path)).ok()?;
    if !asset.starts_with(&root) || !asset.is_file() {
        return None;
    }
    let mut file = std::fs::File::open(asset).ok()?;
    let mut bytes = Vec::with_capacity(MAX_PLUGIN_ICON_BYTES + 1);
    file.by_ref()
        .take((MAX_PLUGIN_ICON_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > MAX_PLUGIN_ICON_BYTES {
        return None;
    }
    sanitize_plugin_icon_svg(&bytes)
}

/// Accept only `#rgb[a]`/`#rrggbb[aa]` or a CodeTerm theme token, so a manifest color can never carry CSS syntax.
pub fn sanitize_plugin_icon_color(value: &str) -> Option<String> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        let valid =
            matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit());
        return valid.then(|| value.to_string());
    }
    let token = value.strip_prefix("var(--ct-")?.strip_suffix(')')?;
    let valid = !token.is_empty()
        && token.len() <= 48
        && token
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    valid.then(|| value.to_string())
}

pub fn sanitize_plugin_icon_svg(bytes: &[u8]) -> Option<String> {
    sanitize_svg(bytes, SvgUse::PluginIcon)
}

pub(crate) fn valid_relative_svg_path(path: &str) -> bool {
    !path.is_empty()
        && path == path.trim()
        && path.to_ascii_lowercase().ends_with(".svg")
        && !path.contains('\0')
        && !path.contains('\\')
        && !path.contains(':')
        && !path.contains("://")
        && !path.to_ascii_lowercase().starts_with("data:")
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

#[cfg(test)]
mod tests;
