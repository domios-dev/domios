use serde_json::json;

use super::*;

fn parse(value: Value) -> InputArgs {
    serde_json::from_value(value).expect("input args")
}

#[test]
fn browser_input_click_moves_presses_and_releases_at_the_point() {
    let commands = input_commands(&InputArgs::click(10.0, 20.5)).unwrap();
    let kinds: Vec<&str> = commands
        .iter()
        .map(|(_, params)| params["type"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["mouseMoved", "mousePressed", "mouseReleased"]);
    assert!(commands
        .iter()
        .all(|(method, params)| *method == "Input.dispatchMouseEvent"
            && params["x"] == json!(10.0)
            && params["y"] == json!(20.5)
            && params["button"] == "left"
            && params["clickCount"] == 1));
}

#[test]
fn browser_input_accepts_the_frontend_click_and_wheel_shapes() {
    let click =
        parse(json!({ "kind": "click", "x": 1, "y": 2, "button": "left", "clickCount": 2 }));
    assert_eq!(input_commands(&click).unwrap()[1].1["clickCount"], 2);
    let wheel = parse(json!({ "kind": "wheel", "x": 1, "y": 2, "deltaX": 3, "deltaY": -40 }));
    let commands = input_commands(&wheel).unwrap();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].1["type"], "mouseWheel");
    assert_eq!(commands[0].1["deltaY"], json!(-40.0));
    assert_eq!(commands[0].1["deltaX"], json!(3.0));
}

#[test]
fn browser_input_text_inserts_and_missing_fields_refuse() {
    let text = parse(json!({ "kind": "text", "text": "héllo" }));
    assert_eq!(
        input_commands(&text).unwrap(),
        vec![("Input.insertText", json!({ "text": "héllo" }))]
    );
    assert!(input_commands(&parse(json!({ "kind": "text" }))).is_err());
    assert!(input_commands(&parse(json!({ "kind": "click", "x": 1 }))).is_err());
    assert!(input_commands(&parse(json!({ "kind": "drag" }))).is_err());
}

#[test]
fn browser_input_agent_enter_submits_with_a_carriage_return() {
    let commands = input_commands(&InputArgs::key("Enter", &["shift".into()])).unwrap();
    assert_eq!(commands[0].1["type"], "keyDown");
    assert_eq!(commands[0].1["text"], "\r");
    assert_eq!(commands[0].1["windowsVirtualKeyCode"], 13);
    assert_eq!(commands[0].1["modifiers"], 8);
    assert_eq!(commands[1].1["type"], "keyUp");
}

#[test]
fn browser_input_editing_keys_are_raw_with_virtual_codes_and_modifier_bits() {
    let commands =
        input_commands(&InputArgs::key("Backspace", &["ctrl".into(), "alt".into()])).unwrap();
    assert_eq!(commands[0].1["type"], "rawKeyDown");
    assert_eq!(commands[0].1["windowsVirtualKeyCode"], 8);
    assert_eq!(commands[0].1["modifiers"], 3);
    assert!(commands[0].1.get("text").is_none());
    assert_eq!(virtual_key_code("F13"), 0);
    let select_all = input_commands(&InputArgs::key("a", &["ctrl".into()])).unwrap();
    assert_eq!(select_all[0].1["windowsVirtualKeyCode"], 65);
    assert_eq!(select_all[0].1["code"], "KeyA");
    assert_eq!(key_code("7"), "Digit7");
    assert_eq!(key_code("Enter"), "Enter");
}

#[test]
fn browser_input_frontend_key_fields_win_over_derived_ones() {
    let key = parse(json!({
        "kind": "key", "key": "a", "code": "KeyA", "windowsVirtualKeyCode": 65, "modifiers": 2, "text": "a"
    }));
    let commands = input_commands(&key).unwrap();
    assert_eq!(commands[0].1["type"], "keyDown");
    assert_eq!(commands[0].1["code"], "KeyA");
    assert_eq!(commands[0].1["windowsVirtualKeyCode"], 65);
    assert_eq!(commands[0].1["modifiers"], 2);
    assert_eq!(commands[0].1["text"], "a");
}

#[test]
fn browser_goto_href_keeps_urls_and_searches_free_text() {
    assert_eq!(goto_href("https://a.dev/x").unwrap(), "https://a.dev/x");
    assert_eq!(
        goto_href("file:///tmp/x.html").unwrap(),
        "file:///tmp/x.html"
    );
    assert_eq!(goto_href("example.com").unwrap(), "https://example.com");
    assert_eq!(goto_href("about:blank").unwrap(), "about:blank");
    assert_eq!(
        goto_href("data:text/html,<p>x</p>").unwrap(),
        "data:text/html,<p>x</p>"
    );
    assert_eq!(
        goto_href("localhost:5173/app").unwrap(),
        "http://localhost:5173/app"
    );
    assert_eq!(
        goto_href("rust borrow checker").unwrap(),
        "https://www.google.com/search?q=rust+borrow+checker"
    );
    assert!(goto_href("  ").is_err());
    assert_eq!(NavAction::parse("back"), Ok(NavAction::Back));
    assert!(NavAction::parse("jump").is_err());
}

#[test]
fn browser_history_entry_steps_back_and_forward_within_bounds() {
    let history = json!({
        "currentIndex": 1,
        "entries": [{ "id": 10 }, { "id": 11 }, { "id": 12 }]
    });
    assert_eq!(history_entry(&history, true), Some(10));
    assert_eq!(history_entry(&history, false), Some(12));
    let first = json!({ "currentIndex": 0, "entries": [{ "id": 10 }] });
    assert_eq!(history_entry(&first, true), None);
    assert_eq!(history_entry(&first, false), None);
}

#[test]
fn browser_quad_center_averages_the_first_quad() {
    let result = json!({ "quads": [[0, 0, 10, 0, 10, 20, 0, 20], [100, 100, 0, 0, 0, 0, 0, 0]] });
    assert_eq!(quad_center(&result), Some((5.0, 10.0)));
    assert_eq!(quad_center(&json!({ "quads": [] })), None);
    assert_eq!(quad_center(&json!({ "quads": [[1, 2]] })), None);
}
