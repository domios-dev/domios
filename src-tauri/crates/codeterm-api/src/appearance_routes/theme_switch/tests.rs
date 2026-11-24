use super::*;
use serde_json::json;

fn builtin(name: &str) -> ThemeConfig {
    get_any_theme(name).unwrap()
}

#[test]
fn switch_bypasses_the_contrast_floor_but_proposals_and_custom_lab_edits_do_not() {
    let mut config = AppConfig::default();
    let previous = AppearanceLook::from_config(&config);
    let mut look = previous.clone();
    look.theme.foreground.clone_from(&look.theme.background);
    let mut proposal = AppearanceProposal {
        id: "test".into(),
        rationale: "Test look".into(),
        tweak: false,
        look,
        previous: previous.clone(),
        previous_active: None,
        state: AppearanceState::Proposed,
        applied_at: None,
    };

    assert_eq!(
        validate(&proposal.look).unwrap_err().0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        apply_proposal(&mut config, &mut proposal).unwrap_err().0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(AppearanceLook::from_config(&config), previous);
    apply_look(&mut config, &mut proposal, ContrastPolicy::NamedTheme).unwrap();
    assert_eq!(AppearanceLook::from_config(&config), proposal.look);
}

#[test]
fn builtin_light_themes_pass_every_appearance_contrast_floor() {
    for name in ["github-light", "solarized-light"] {
        let look = AppearanceLook {
            theme: builtin(name),
            wallpaper: None,
            tuning: None,
        };
        let checks = measure(&look).unwrap();
        assert!(!checks.is_empty());
        for check in checks {
            assert!(
                check.ratio >= check.floor,
                "{name}: {} {:.2} < {}",
                check.pair,
                check.ratio,
                check.floor
            );
        }
    }
}

#[test]
fn switches_keep_recolorable_wallpaper_and_replace_baked_colors_only_on_a_luminance_flip() {
    let dark = builtin("github-dark");
    let light = builtin("github-light");
    let forest = palette_preset("Forest Night").unwrap();
    let stroke = r#"<svg viewBox="0 0 8 8"><g fill="none" stroke="currentColor"><path d="M0 0L8 8"/></g></svg>"#;
    let fill = r#"<svg viewBox="0 0 8 8" stroke="currentColor"><path d="M0 0L8 8"/></svg>"#;
    let asset = format!("{}.svg", "a".repeat(64));
    let samples = [
        (json!({"background":{"pattern":"none"}}), true),
        (json!({"background":{"pattern":"dots"}}), true),
        (
            json!({"background":{"pattern":"botanical","opacity":0.16},"fill":{"mode":"gradient","preset":"midnight"}}),
            true,
        ),
        (
            json!({"background":{"pattern":"svg","opacity":0.16},"asset":asset}),
            true,
        ),
        (json!({"background":{"pattern":"svg","svg":fill}}), false),
        (
            json!({"background":{"pattern":"svg","svg":stroke, "gradient":{"kind":"linear","stops":["#0d1117","#141b33"]}}}),
            false,
        ),
        (
            json!({"background":{"pattern":"image"},"asset":format!("{}.png","a".repeat(64))}),
            false,
        ),
        (
            json!({"background":{"pattern":"botanical"},"fill":{"mode":"gradient","preset":"midnight"},"fill_strength":0.4}),
            false,
        ),
    ];
    for (value, keep) in samples {
        let paper: ChatWallpaper = serde_json::from_value(value).unwrap();
        let read_svg = |id: &str| {
            assert_eq!(id, asset);
            Some(stroke.as_bytes().to_vec())
        };
        let current = AppearanceLook {
            theme: dark.clone(),
            wallpaper: Some(paper.clone()),
            tuning: None,
        };
        assert_eq!(
            theme_look(&current, forest.clone(), |_| panic!(
                "same luminance needs no SVG read"
            ))
            .wallpaper,
            Some(paper.clone())
        );
        let expected = keep.then_some(paper);
        assert_eq!(
            theme_look(&current, light.clone(), read_svg).wallpaper,
            expected
        );
        let reverse = AppearanceLook {
            theme: light.clone(),
            ..current
        };
        assert_eq!(
            theme_look(&reverse, dark.clone(), read_svg).wallpaper,
            expected
        );
    }
}
