//! Override group inheritance. Split out of `model_catalog.rs` to keep that
//! file under the 1200-line source limit (AGENTS.md).

use super::ModelInfo;

/// An override entry replaces its built-in wholesale, so a user file that
/// doesn't restate `group` must not silently ungroup a model the provider
/// placed. `replace = true` still drops built-ins the file never names.
pub(super) fn inherit_provider_groups(merged: &mut [ModelInfo], provider: &[ModelInfo]) {
    let provider_groups: std::collections::HashMap<&str, &str> = provider
        .iter()
        .filter_map(|m| m.group.as_deref().map(|g| (m.id.as_str(), g)))
        .collect();
    for model in merged.iter_mut() {
        if model.group.is_none() {
            if let Some(group) = provider_groups.get(model.id.as_str()) {
                model.group = Some((*group).to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(id: &str, display: &str) -> ModelInfo {
        ModelInfo {
            id: id.into(),
            display_name: display.into(),
            ..ModelInfo::default()
        }
    }

    #[test]
    fn override_without_group_inherits_the_provider_group() {
        let provider = vec![
            ModelInfo {
                group: Some("deepseek".into()),
                ..model("deepseek-v4-flash", "DeepSeek V4 Flash")
            },
            model("plain", "Plain"),
        ];
        let mut merged = vec![
            ModelInfo {
                badge: Some("own".into()),
                ..model("deepseek-v4-flash", "DeepSeek V4 Flash (override)")
            },
            model("plain", "Plain"),
            model("added-by-user", "Added"),
        ];

        inherit_provider_groups(&mut merged, &provider);

        assert_eq!(
            merged[0].group.as_deref(),
            Some("deepseek"),
            "an override that omits group keeps the provider's section"
        );
        assert_eq!(
            merged[1].group, None,
            "a model the provider never grouped stays flat"
        );
        assert_eq!(
            merged[2].group, None,
            "a user-added model is not guessed into a section"
        );
    }
}
