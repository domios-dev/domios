use super::*;

fn capabilities() -> Vec<Capability> {
    vec![Capability {
        id: "opaque-id".into(),
        purpose: "OPAQUE_PURPOSE".into(),
    }]
}

#[test]
fn first_week_window_and_opt_out() {
    let input = capabilities();
    let start = 100;
    assert!(build(&input, start, start, true).is_some());
    assert!(build(&input, start + FIRST_WEEK_SECONDS - 1, start, true).is_some());
    assert_eq!(build(&input, start + FIRST_WEEK_SECONDS, start, true), None);
    assert_eq!(
        build(&input, start + FIRST_WEEK_SECONDS + 1, start, true),
        None
    );
    assert_eq!(build(&input, start, start, false), None);
}

#[test]
fn bounded_single_lines_and_stable_output() {
    let input: Vec<_> = (0..100)
        .map(|i| Capability {
            id: format!("opaque-{i}"),
            purpose: "界\n\r".repeat(500),
        })
        .collect();
    let fragment = build(&input, 0, 0, true).unwrap();
    assert!(fragment.len() <= MAX_FRAGMENT_BYTES);
    let lines: Vec<_> = fragment
        .lines()
        .filter(|line| line.starts_with("- "))
        .collect();
    assert_eq!(lines.len(), MAX_CAPABILITIES);
    assert!(lines.iter().all(|line| line.len() <= MAX_LINE_BYTES + 2));
    assert_eq!(Some(fragment), build(&input, 0, 0, true));
}
