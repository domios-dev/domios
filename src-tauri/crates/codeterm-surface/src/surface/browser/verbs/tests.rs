use super::*;

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_string()).collect()
}

#[test]
fn browser_open_args_take_a_url_and_an_optional_profile() {
    assert_eq!(parse_open(&[]), (None, None));
    assert_eq!(
        parse_open(&args(&["https://a.dev", "--profile", "work"])),
        (Some("https://a.dev".into()), Some("work".into()))
    );
    assert_eq!(
        parse_open(&args(&["--url", "a.dev"])),
        (Some("a.dev".into()), None)
    );
}

#[test]
fn browser_wait_args_need_a_flag_and_a_value() {
    assert_eq!(
        parse_wait(&args(&["--text", "Signed", "in"])),
        Ok(WaitFor::Text("Signed in".into()))
    );
    assert_eq!(
        parse_wait(&args(&["--url", "/done"])),
        Ok(WaitFor::Url("/done".into()))
    );
    assert_eq!(
        parse_wait(&args(&["--selector", "#ok"])),
        Ok(WaitFor::Selector("#ok".into()))
    );
    assert!(parse_wait(&args(&["--text"])).is_err());
    assert!(parse_wait(&args(&["--load", "x"])).is_err());
    assert!(parse_wait(&[]).is_err());
}

#[test]
fn browser_wait_expression_embeds_the_value_as_a_json_literal_and_a_failure_deadline() {
    let expression = wait_expression(&WaitFor::Text("say \"hi\"".into())).unwrap();
    assert!(expression.contains(r#"innerText.includes("say \"hi\"")"#));
    assert!(expression.contains("MutationObserver"));
    assert!(expression.contains(&format!("}}, {})", WAIT_DEADLINE.as_millis())));
    let selector = wait_expression(&WaitFor::Selector("a[href='x']".into())).unwrap();
    assert!(selector.contains(r#"querySelector("a[href='x']")"#));
    assert!(wait_expression(&WaitFor::Url("x".into())).is_none());
}

#[test]
fn browser_verb_limits_fall_back_on_bad_input() {
    assert_eq!(parse_limit(Some(&"50".to_string()), 200), 50);
    assert_eq!(parse_limit(Some(&"0".to_string()), 200), 200);
    assert_eq!(parse_limit(Some(&"many".to_string()), 200), 200);
    assert_eq!(parse_limit(None, 4000), 4000);
    assert!(usage_args(&[], "goto <url>").is_err());
    assert!(VERBS.contains(&"inbox"));
}
