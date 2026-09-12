//! The catalogue-backed short name a recognised model renders as, stated once
//! and mirrored by `src/lib/agentModelName.ts`.
//!
//! The provider catalogues (`plugins/provider-*/plugin.json`) own the model
//! ids; this module owns the one policy that turns such an id into the short
//! family a bracket shows. The policy is structural so it survives a version
//! bump — `gpt-5.6-sol` and a later `gpt-5.9-sol` both render `sol` — and it is
//! closed over [`MODEL_FAMILIES`] so an id no catalogue carries is reported
//! unrecognised instead of guessed at. `catalogue_ids_all_resolve` keeps the
//! two in step: a new catalogue entry fails it until its family is listed.

/// Codenames the shipped catalogues carry, lowercased. Version-free by design.
pub const MODEL_FAMILIES: &[&str] = &[
    // provider-claude
    "opus", "sonnet", "haiku", "fable", // provider-codex
    "astra", "sol", "terra", "luna", "spark", "mini",
];

/// Vendors whose catalogue carries un-codenamed entries (`gpt-5.5`), rendered
/// as `<vendor> <version>` because the version IS the identity there.
const VERSIONED_VENDORS: &[&str] = &["gpt"];

/// Tokens that name the product line rather than the model, so they must not
/// be mistaken for a family or block a `<vendor> <version>` read.
const NOISE_TOKENS: &[&str] = &["codex"];

fn tokenize(model: &str) -> Vec<String> {
    let lowered = model.trim().to_ascii_lowercase();
    let body = lowered.rsplit('/').next().unwrap_or("");
    body.split(|c: char| c == '-' || c == '_' || c.is_whitespace())
        .filter(|token| !token.is_empty() && !NOISE_TOKENS.contains(token))
        .map(str::to_string)
        .collect()
}

fn is_version(token: &str) -> bool {
    token.chars().any(|c| c.is_ascii_digit())
        && token.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// The short name for a catalogue model, or `None` when no catalogue claims it.
/// Pure: it only decides what to render and never speaks for the stored value.
pub fn short_model_name(model: &str) -> Option<String> {
    let tokens = tokenize(model);
    if let Some(family) = tokens
        .iter()
        .find(|token| MODEL_FAMILIES.contains(&token.as_str()))
    {
        return Some(family.clone());
    }
    let (vendor, version) = tokens.split_first()?;
    if !VERSIONED_VENDORS.contains(&vendor.as_str()) || version.is_empty() {
        return None;
    }
    version
        .iter()
        .all(|token| is_version(token))
        .then(|| format!("{vendor} {}", version.join(".")))
}

#[cfg(test)]
mod tests;
