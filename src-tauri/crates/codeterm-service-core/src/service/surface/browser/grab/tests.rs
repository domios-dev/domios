use serde_json::json;

use super::*;

fn collected() -> GrabTarget {
    let mut target: GrabTarget = serde_json::from_value(json!({
        "tagName": "button",
        "selector": "#login > button.primary",
        "text": "Sign in",
        "classes": "primary big",
        "attributes": [["type", "submit"], ["aria-label", "Sign in"]],
        "styles": [["color", "rgb(0, 0, 0)"], ["display", "block"]],
        "react": "LoginButton < Form < App",
        "source": "src/Login.tsx:42",
        "nearbyText": ["Forgot password?"],
        "ancestorPath": ["main", "form#login"],
        "rectViewport": { "x": 1, "y": 2, "width": 3, "height": 4 },
        "rectPage": { "x": 10.4, "y": 20.6, "width": 120.2, "height": 32 },
        "page": { "url": "https://a.dev/login", "title": "Login", "viewport": "1600x900" }
    }))
    .expect("collected target");
    target.accessibility = Accessibility {
        role: Some("button".into()),
        name: Some("Sign in".into()),
    };
    target
}

#[test]
fn browser_grab_label_prefers_component_and_accessible_name() {
    assert_eq!(
        element_label(&collected()),
        "LoginButton button \"Sign in\""
    );
    let mut plain = collected();
    plain.react = None;
    plain.accessibility = Accessibility::default();
    assert_eq!(element_label(&plain), "button \"Sign in\"");
    plain.text.clear();
    assert_eq!(element_label(&plain), "button");
}

#[test]
fn browser_annotation_markdown_carries_every_collected_fact() {
    let target = collected();
    let markdown = annotation_markdown(&Annotation {
        target: &target,
        intent: "bug",
        comment: "it does nothing",
        screenshot_path: Some("/pad/screenshots/g1.png"),
        sender: None,
    });
    let lines: Vec<&str> = markdown.lines().collect();
    assert_eq!(lines[0], "## Browser annotation from the user");
    assert_eq!(
        lines[1],
        "**Page:** Login — `https://a.dev/login` (viewport 1600x900)"
    );
    assert_eq!(lines[3], "### LoginButton button \"Sign in\"");
    for expected in [
        "**Intent:** bug",
        "**User says:** it does nothing",
        "**Selector:** `#login > button.primary`",
        "**Accessibility:** role `button`, name \"Sign in\"",
        "**React:** LoginButton < Form < App",
        "**Source:** `src/Login.tsx:42`",
        "**Text:** \"Sign in\"",
        "**Attributes:** type=\"submit\" aria-label=\"Sign in\"",
        "**Classes:** `primary big`",
        "**Box:** 10,21 120×32",
        "**Computed styles:** color: rgb(0, 0, 0); display: block",
        "**Nearby text:** \"Forgot password?\"",
        "**Inside:** main > form#login",
        "**Element screenshot:** `/pad/screenshots/g1.png`",
    ] {
        assert!(
            lines.contains(&expected),
            "missing {expected:?} in\n{markdown}"
        );
    }
}

#[test]
fn browser_annotation_markdown_omits_absent_facts() {
    let target = GrabTarget {
        tag_name: "div".into(),
        ..Default::default()
    };
    let markdown = annotation_markdown(&Annotation {
        target: &target,
        intent: "question",
        comment: " ",
        screenshot_path: None,
        sender: None,
    });
    assert!(markdown.contains("**User says:** (no comment)"));
    for absent in [
        "**React:**",
        "**Attributes:**",
        "**Element screenshot:**",
        "**Inside:**",
    ] {
        assert!(!markdown.contains(absent), "{absent} present");
    }
}

#[test]
fn browser_grab_accessibility_skips_ignored_nodes() {
    let tree = json!({ "nodes": [
        { "ignored": true, "role": { "value": "generic" } },
        { "role": { "value": "link" }, "name": { "value": "Home" } }
    ]});
    assert_eq!(
        accessibility_of(&tree),
        Accessibility {
            role: Some("link".into()),
            name: Some("Home".into())
        }
    );
    assert_eq!(accessibility_of(&json!({})), Accessibility::default());
}

#[test]
fn browser_grab_intent_and_clip_are_bounded() {
    assert_eq!(parse_intent(None), Ok("question"));
    assert_eq!(parse_intent(Some("change")), Ok("change"));
    assert!(parse_intent(Some("delete")).is_err());
    let clip = screenshot_clip(&Rect {
        x: 1.0,
        y: 2.0,
        width: 5000.0,
        height: 10.0,
    })
    .unwrap();
    assert_eq!(clip["width"], json!(1600.0));
    assert!(screenshot_clip(&Rect::default()).is_none());
    let card = collected().to_json();
    assert_eq!(card["attributes"]["type"], "submit");
    assert_eq!(card["accessibility"]["role"], "button");
}

#[test]
fn browser_annotation_markdown_names_the_annotating_agent() {
    let target = collected();
    let markdown = annotation_markdown(&Annotation {
        target: &target,
        intent: "bug",
        comment: "the button is clipped",
        screenshot_path: None,
        sender: Some("QA"),
    });
    let lines: Vec<&str> = markdown.lines().collect();
    assert_eq!(lines[0], "## Browser annotation from QA");
    assert!(markdown.contains("**QA says:** the button is clipped"));
    assert!(!markdown.contains("the user"));
    assert!(markdown.contains("**Selector:** `#login > button.primary`"));
}

#[test]
fn browser_annotate_args_take_a_click_locator_a_comment_and_an_optional_intent() {
    let words = |items: &[&str]| {
        items
            .iter()
            .map(|item| item.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        parse_annotate(&words(&["@e3", "too small"])),
        Ok(AnnotateRequest {
            locator: words(&["@e3"]),
            comment: "too small".into(),
            intent: "question",
        })
    );
    assert_eq!(
        parse_annotate(&words(&["button", "Sign in", "clipped", "--intent", "bug"])),
        Ok(AnnotateRequest {
            locator: words(&["button", "Sign in"]),
            comment: "clipped".into(),
            intent: "bug",
        })
    );
    for bad in [
        &[][..],
        &["@e3"][..],
        &["@e3", " "][..],
        &["@e3", "x", "--intent"][..],
    ] {
        assert!(parse_annotate(&words(bad)).is_err(), "{bad:?}");
    }
    assert!(parse_annotate(&words(&["@e3", "x", "--intent", "rant"])).is_err());
}
