use super::*;

fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(|s| (*s).into()).collect()
}

#[test]
fn browser_interaction_target_validation() {
    for verb in ["hover", "double-click", "triple-click", "right-click"] {
        assert!(parse(verb, &args(&["@capture:1"])).is_ok());
        assert!(parse(verb, &args(&["button", "Save"])).is_ok());
        for words in [
            vec![],
            vec!["button"],
            vec!["--unknown", "Save"],
            vec!["button", "Save", "extra"],
        ] {
            assert!(parse(verb, &args(&words)).is_err(), "{verb} {words:?}");
        }
    }
    assert!(parse("scroll-to", &args(&["@capture:1"])).is_ok());
    assert!(parse("scroll-to", &args(&["button", "Save"])).is_err());
}

#[test]
fn browser_interaction_drag_validation() {
    assert!(parse("drag", &args(&["@a", "@b"])).is_ok());
    assert_eq!(
        parse("drag", &args(&["1", "2", "3", "4"])).unwrap(),
        Interaction::DragPoints([1.0, 2.0, 3.0, 4.0])
    );
    for words in [
        vec!["@a"],
        vec!["1", "2"],
        vec!["NaN", "0", "1", "1"],
        vec!["-1", "0", "1", "1"],
        vec!["0", "0", "inf", "1"],
    ] {
        assert!(parse("drag", &args(&words)).is_err());
    }
}

#[test]
fn browser_interaction_key_and_form_validation() {
    assert_eq!(
        parse("key", &args(&["Enter", "ctrl", "--repeat", "3"])).unwrap(),
        Interaction::Key("Enter".into(), vec!["ctrl".into()], 3)
    );
    for words in [
        vec![],
        vec!["Enter", "--repeat"],
        vec!["Enter", "--repeat", "0"],
        vec!["Enter", "--repeat", "101"],
        vec!["Enter", "unknown"],
        vec!["Enter", "--repeat", "2", "--repeat", "2"],
    ] {
        assert!(parse("key", &args(&words)).is_err());
    }
    assert!(parse("form-value", &args(&["@a", "true"])).is_ok());
    assert!(parse("form-value", &args(&["@a", ""])).is_ok());
    assert!(parse("form-value", &args(&["@a"])).is_err());
    assert!(parse("form-value", &args(&["textbox", "x"])).is_err());
}
