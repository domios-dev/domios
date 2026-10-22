use super::*;
use crate::service::inject_verify::InjectionError;

#[test]
fn codex_rpc_send_batch_routes_each_item_once_without_paste() {
    let batch = ["first payload", "second payload"]
        .into_iter()
        .map(|text| (text.to_string(), None))
        .collect();
    let mut calls = Vec::new();
    let result: Result<(), String> = execute_pty_batch(
        batch,
        |_| panic!("unexpected deadline"),
        |text| {
            crate::service::codex_rpc::send::route_or_pty(
                Some({
                    calls.push(text.to_string());
                    Ok(())
                }),
                || panic!("RPC batch must not paste"),
            )
        },
    );
    assert_eq!(result, Ok(()));
    assert_eq!(calls, ["first payload", "second payload"]);
}

#[test]
fn codex_rpc_send_batch_stops_at_uncertainty_without_replaying_or_sending_next() {
    let batch = ["first payload", "second payload"]
        .into_iter()
        .map(|text| (text.to_string(), None))
        .collect();
    let mut calls = Vec::new();
    let result = execute_pty_batch(
        batch,
        |_| panic!("unexpected deadline"),
        |text| {
            calls.push(text.to_string());
            crate::service::codex_rpc::send::route_or_pty(Some(Err("submit_unconfirmed")), || {
                panic!("RPC uncertainty must not paste")
            })
        },
    );
    assert_eq!(result, Err("submit_unconfirmed"));
    assert_eq!(calls, ["first payload"]);
}

#[test]
fn batch_submit_failure_is_not_detached_or_erased() {
    let batch = vec![("parked".to_string(), Some(5))];
    let mut deadlines = Vec::new();
    let result = execute_pty_batch(
        batch,
        |deadline| deadlines.push(deadline),
        |_| {
            Err::<(), _>(InjectionError::SubmitNeverConfirmed {
                waited: std::time::Duration::ZERO,
                screen_tail: "> parked".into(),
            })
        },
    );
    assert!(matches!(
        result,
        Err(InjectionError::SubmitNeverConfirmed { .. })
    ));
    assert_eq!(deadlines, [std::time::Duration::from_millis(5)]);
}
