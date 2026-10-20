use super::*;

fn model(id: &str) -> ModelInfo {
    ModelInfo {
        id: id.to_string(),
        display_name: id.to_string(),
        ..ModelInfo::default()
    }
}

fn unavailable(id: &str) -> ModelInfo {
    ModelInfo {
        available: Some(false),
        ..model(id)
    }
}

fn authoritative<'a>(models: &'a [ModelInfo], recommended: Option<&'a str>) -> CatalogueView<'a> {
    CatalogueView {
        models,
        recommended,
        authoritative: true,
        previously_listed: &[],
    }
}

#[test]
fn offered_default_is_kept() {
    let models = [model("gpt-6-sol"), model("gpt-5.5")];
    assert_eq!(
        replacement_for("GPT-6-SOL", &authoritative(&models, None)),
        None
    );
}

#[test]
fn empty_default_is_kept() {
    let models = [model("gpt-6-sol")];
    assert_eq!(replacement_for("  ", &authoritative(&models, None)), None);
}

#[test]
fn display_name_and_context_suffix_count_as_offered() {
    let models = [ModelInfo {
        display_name: "Sonnet".into(),
        ..model("sonnet")
    }];
    assert_eq!(
        replacement_for("Sonnet", &authoritative(&models, None)),
        None
    );
    assert_eq!(
        replacement_for("sonnet[1m]", &authoritative(&models, None)),
        None
    );
}

#[test]
fn removed_model_moves_to_latest_version_in_its_family() {
    let models = [
        model("gpt-5.1-codex-mini"),
        model("gpt-5.2-codex"),
        model("gpt-5.3-codex"),
        model("gpt-6-sol"),
    ];
    assert_eq!(
        replacement_for("gpt-5.1-codex", &authoritative(&models, Some("gpt-6-sol"))),
        Some(Replacement {
            model: "gpt-5.3-codex".into(),
            reason: ReplacementReason::SameFamilyLatest,
        })
    );
}

#[test]
fn dated_snapshot_moves_to_newest_family_member() {
    let models = [
        model("claude-opus-4-1-20250805"),
        model("claude-opus-5-5"),
        model("claude-sonnet-5-5"),
    ];
    assert_eq!(
        replacement_for("claude-opus-4-20250514", &authoritative(&models, None))
            .map(|replacement| replacement.model),
        Some("claude-opus-5-5".into())
    );
}

#[test]
fn routed_ids_keep_their_upstream_prefix_in_the_family() {
    let models = [
        model("anthropic/claude-sonnet-5-5"),
        model("openai/claude-sonnet-9"),
    ];
    assert_eq!(
        replacement_for("anthropic/claude-sonnet-4-5", &authoritative(&models, None))
            .map(|replacement| replacement.model),
        Some("anthropic/claude-sonnet-5-5".into())
    );
}

#[test]
fn unavailable_models_are_never_chosen() {
    let models = [
        unavailable("gpt-5.4-codex"),
        model("gpt-5.2-codex"),
        model("gpt-6-sol"),
    ];
    assert_eq!(
        replacement_for("gpt-5.1-codex", &authoritative(&models, None))
            .map(|replacement| replacement.model),
        Some("gpt-5.2-codex".into())
    );
}

#[test]
fn unavailable_default_counts_as_removed() {
    let models = [unavailable("gpt-5.1-codex"), model("gpt-6-sol")];
    assert_eq!(
        replacement_for("gpt-5.1-codex", &authoritative(&models, Some("gpt-6-sol"))),
        Some(Replacement {
            model: "gpt-6-sol".into(),
            reason: ReplacementReason::ProviderRecommended,
        })
    );
}

#[test]
fn no_family_member_falls_back_to_the_recommended_model() {
    let models = [model("gpt-6-sol"), model("gpt-5.5")];
    assert_eq!(
        replacement_for("haiku", &authoritative(&models, Some("gpt-5.5"))),
        Some(Replacement {
            model: "gpt-5.5".into(),
            reason: ReplacementReason::ProviderRecommended,
        })
    );
}

#[test]
fn recommended_model_that_is_itself_gone_is_skipped() {
    let models = [model("gpt-6-sol")];
    assert_eq!(
        replacement_for("haiku", &authoritative(&models, Some("gpt-5.5"))),
        Some(Replacement {
            model: "gpt-6-sol".into(),
            reason: ReplacementReason::FirstAvailable,
        })
    );
}

#[test]
fn empty_catalogue_never_replaces() {
    let models = [unavailable("gpt-6-sol")];
    assert_eq!(
        replacement_for("gpt-5.5", &authoritative(&models, Some("gpt-6-sol"))),
        None
    );
    assert_eq!(replacement_for("gpt-5.5", &authoritative(&[], None)), None);
}

#[test]
fn free_form_catalogue_keeps_an_id_it_never_listed() {
    let models = [model("anthropic/claude-sonnet-5-5")];
    let view = CatalogueView {
        models: &models,
        recommended: None,
        authoritative: false,
        previously_listed: &[],
    };
    assert_eq!(replacement_for("local/my-custom-model", &view), None);
}

#[test]
fn free_form_catalogue_replaces_an_id_it_stopped_listing() {
    let models = [model("anthropic/claude-sonnet-5-5")];
    let previously_listed = ["anthropic/claude-sonnet-4-5".to_string()];
    let view = CatalogueView {
        models: &models,
        recommended: None,
        authoritative: false,
        previously_listed: &previously_listed,
    };
    assert_eq!(
        replacement_for("anthropic/claude-sonnet-4-5", &view),
        Some(Replacement {
            model: "anthropic/claude-sonnet-5-5".into(),
            reason: ReplacementReason::SameFamilyLatest,
        })
    );
}

#[test]
fn family_and_version_split_numeric_segments() {
    assert_eq!(
        family_and_version("gpt-5.1-codex"),
        ("gpt-codex".into(), vec![5, 1])
    );
    assert_eq!(family_and_version("gpt-6-sol"), ("gpt-sol".into(), vec![6]));
    assert_eq!(family_and_version("sonnet"), ("sonnet".into(), vec![]));
}
