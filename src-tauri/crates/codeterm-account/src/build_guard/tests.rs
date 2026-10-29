use super::*;

#[test]
fn release_requires_every_public_config_value() {
    let configured = VARIABLES.map(|name| Some(format!("fixture-{name}")));
    assert!(validate(true, &configured).is_ok());
    for index in [0, 2, 3] {
        for absent in [None, Some(" \t".into())] {
            let mut values = configured.clone();
            values[index] = absent;
            let error = validate(true, &values).unwrap_err();
            assert!(error.contains(VARIABLES[index]));
            assert!(!error.contains("fixture-"));
        }
    }
    assert!(validate(false, &[None, None, None, None]).is_ok());
}

#[test]
fn auth_domain_defaults_to_project_and_preserves_explicit_value() {
    let mut values = [
        Some("fixture-key".into()),
        None,
        Some("fixture-project".into()),
        Some("fixture-app".into()),
    ];
    assert!(validate(true, &values).is_ok());
    resolve_auth_domain(&mut values);
    assert_eq!(
        values[1].as_deref(),
        Some("fixture-project.firebaseapp.com")
    );
    values[1] = Some("fixture.invalid".into());
    resolve_auth_domain(&mut values);
    assert_eq!(values[1].as_deref(), Some("fixture.invalid"));
}

#[test]
fn fingerprint_tracks_every_config_value_and_field_boundary() {
    let original = VARIABLES.map(|name| Some(name.to_owned()));
    for index in 0..VARIABLES.len() {
        let mut changed = original.clone();
        changed[index] = Some("fixture-change".into());
        assert_ne!(fingerprint(&original), fingerprint(&changed));
    }
    assert_ne!(
        fingerprint(&[Some("ab".into()), Some("c".into()), None, None]),
        fingerprint(&[Some("a".into()), Some("bc".into()), None, None])
    );
}
