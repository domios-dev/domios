use super::*;

#[test]
fn single_open_attempt_resolves_its_selected_id() {
    assert_eq!(resolve_open_attempt("task", &["own".into()]), Ok("own"));
}

#[test]
fn no_open_attempt_refuses_with_task_and_empty_candidates() {
    assert_eq!(
        resolve_open_attempt("task", &[]),
        Err(AttemptResolutionRefusal::Missing {
            task: "task".into(),
            candidates: vec![],
        })
    );
}

#[test]
fn several_open_attempts_refuse_with_all_candidates_in_order() {
    assert_eq!(
        resolve_open_attempt("task", &["first".into(), "second".into()]),
        Err(AttemptResolutionRefusal::Ambiguous {
            task: "task".into(),
            candidates: vec!["first".into(), "second".into()],
        })
    );
}
