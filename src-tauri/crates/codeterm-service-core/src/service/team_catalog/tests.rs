use super::super::team_channels::{TeamChannelKind, TeamChannelRow};
use super::*;

fn row(name: &str) -> TeamChannelRow {
    TeamChannelRow {
        name: name.into(),
        display_name: name.into(),
        description: String::new(),
        url: String::new(),
        path: String::new(),
        kind: TeamChannelKind::Local,
        official: name == OFFICIAL,
        last_refresh: None,
        skipped_entries: Vec::new(),
    }
}

#[test]
fn catalog_layers_forward_official_versions_and_saved_templates_keep_their_identity() {
    let root = tempfile::tempdir().unwrap();
    let library = TemplateLibrary::new(root.path());
    let bundled = bundled()[0].1.clone();
    let current = semver::Version::parse(&bundled.document.version).unwrap();
    let newer = format!("{}.0.0", current.major + 1);
    for version in [
        bundled.document.version.clone(),
        "0.0.0".into(),
        format!("{}+build.9", bundled.document.version),
        newer.clone(),
    ] {
        let mut offered = bundled.clone();
        offered.document.version = version.clone();
        let entries = layer(vec![(row(OFFICIAL), offered)], &library);
        let entry = entries
            .iter()
            .find(|entry| entry.package.document.name == bundled.document.name)
            .unwrap();
        assert_eq!(
            entry.package.document.version,
            if version == newer {
                &newer
            } else {
                &bundled.document.version
            }
            .as_str()
        );
        assert_eq!(entry.provenance, TemplateProvenance::Builtin);
        assert_eq!(entry.bundled_version.is_some(), version == newer);
        assert!(super::super::team_template_start::consent_reasons(
            &entry.package,
            entry.provenance
        )
        .is_empty());
    }
    let mut extra = bundled.clone();
    extra.document.name = "dev.domios/extra".into();
    let mut community = bundled.clone();
    community.document.name = "example/community".into();
    let entries = layer(
        vec![
            (row("community"), bundled.clone()),
            (row("community"), community.clone()),
            (row(OFFICIAL), extra),
        ],
        &library,
    );
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.package.document.name == bundled.document.name)
            .count(),
        1
    );
    assert!(entries
        .iter()
        .any(|entry| entry.package.document.name == "dev.domios/extra"
            && entry.provenance == TemplateProvenance::Builtin));
    let untrusted = entries
        .iter()
        .find(|entry| entry.package.document.name == community.document.name)
        .unwrap();
    assert_eq!(untrusted.provenance, TemplateProvenance::Channel);
    assert_eq!(
        untrusted.summary(&library).channel.as_deref(),
        Some("community")
    );
    assert!(
        !super::super::team_template_start::consent_reasons(&community, untrusted.provenance)
            .is_empty()
    );
    library.save(&community, TemplateProvenance::Saved).unwrap();
    let entries = layer(vec![(row("community"), community)], &library);
    assert_eq!(
        entries
            .iter()
            .find(|entry| entry.package.document.name == "example/community")
            .unwrap()
            .provenance,
        TemplateProvenance::Saved
    );
    assert!(library.save(&bundled, TemplateProvenance::Saved).is_err());
}
