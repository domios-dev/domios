use super::SendRejection;
use crate::service::delivery_error::DeliveryError;
use crate::service::inject_verify::InjectionError;
use crate::service::pty_io::{execute_ordered_actions, OrderedPtyAction};
use axum::http::StatusCode;

#[test]
fn owner_keys_into_a_provider_dialog_are_written_raw() {
    let keys = super::owner_dialog_keys(&[
        OrderedPtyAction::Raw(b"down".to_vec()),
        OrderedPtyAction::Submit,
    ]);
    assert_eq!(
        keys,
        vec![
            OrderedPtyAction::Raw(b"down".to_vec()),
            OrderedPtyAction::Raw(b"\r".to_vec()),
        ]
    );
}

#[test]
fn send_actions_enter_into_provider_modal_writes_zero_bytes() {
    let mut bytes_written = 0usize;
    let result = execute_ordered_actions(
        "",
        &[OrderedPtyAction::Submit],
        |body, enter| {
            assert!(body.is_empty());
            assert!(enter);
            Err::<(), _>(DeliveryError::TabBusyRefused {
                tab: "53854ac8".into(),
                detail: "provider update dialog is visible".into(),
            })
        },
        |bytes| {
            bytes_written += bytes.len();
            Ok(())
        },
        |_| {},
    );
    let rejection = SendRejection::from(result.unwrap_err());

    assert_eq!(bytes_written, 0);
    assert_eq!(rejection.status, StatusCode::CONFLICT);
    assert_eq!(rejection.body["error"]["code"], "tab_busy_refused");
    assert_eq!(rejection.body["error"]["bytes_written"], false);
    assert_eq!(
        rejection.body["error"]["detail"],
        "provider update dialog is visible"
    );
}

#[test]
fn send_actions_raw_key_into_provider_modal_writes_zero_bytes() {
    let mut raw_writes = Vec::new();
    let result = execute_ordered_actions(
        "",
        &[OrderedPtyAction::Raw(b"up".to_vec())],
        |_, _| panic!("raw action must not wait for public agent readiness"),
        |bytes| {
            if bytes == b"up" {
                Err(DeliveryError::TabBusyRefused {
                    tab: "53854ac8".into(),
                    detail: "provider trust dialog is visible".into(),
                })
            } else {
                raw_writes.push(bytes.to_vec());
                Ok(())
            }
        },
        |_| {},
    );
    let rejection = SendRejection::from(result.unwrap_err());

    assert!(raw_writes.is_empty());
    assert_eq!(rejection.status, StatusCode::CONFLICT);
    assert_eq!(rejection.body["error"]["code"], "tab_busy_refused");
    assert_eq!(rejection.body["error"]["bytes_written"], false);
    assert_eq!(
        rejection.body["error"]["detail"],
        "provider trust dialog is visible"
    );
}

#[test]
fn api_send_actions_text_enter_uses_acknowledged_submit() {
    let mut submits = Vec::new();
    let mut raw_writes = Vec::new();
    let result = execute_ordered_actions(
        "parked body",
        &[OrderedPtyAction::Submit],
        |text, enter| {
            submits.push((text.to_string(), enter));
            Err::<(), _>(InjectionError::SubmitNeverConfirmed {
                waited: std::time::Duration::ZERO,
                screen_tail: "> parked body".into(),
            })
        },
        |bytes| {
            raw_writes.push(bytes.to_vec());
            Ok(())
        },
        |_| {},
    );
    assert!(matches!(
        result,
        Err(InjectionError::SubmitNeverConfirmed { .. })
    ));
    assert_eq!(submits, [("parked body".into(), true)]);
    assert!(raw_writes.is_empty());
}

#[test]
fn api_send_actions_raw_keys_ordered_bounded() {
    let actions = [
        OrderedPtyAction::Raw(b"up".to_vec()),
        OrderedPtyAction::Delay(std::time::Duration::from_millis(25)),
        OrderedPtyAction::Raw(b"esc".to_vec()),
    ];
    let mut raw_writes = Vec::new();
    let mut deadlines = Vec::new();
    let mut preflights = Vec::new();
    execute_ordered_actions(
        "",
        &actions,
        |text, enter| {
            preflights.push((text.to_string(), enter));
            Ok::<_, String>(())
        },
        |bytes| {
            raw_writes.push(bytes.to_vec());
            Ok(())
        },
        |deadline| deadlines.push(deadline),
    )
    .unwrap();
    assert_eq!(raw_writes, [b"up".to_vec(), b"esc".to_vec()]);
    assert_eq!(deadlines, [std::time::Duration::from_millis(25)]);
    assert!(preflights.is_empty());
}

#[test]
fn send_actions_enter_and_ctrl_c_to_shell_write_their_bytes() {
    let written = std::cell::RefCell::new(Vec::new());
    let result = execute_ordered_actions(
        "",
        &[OrderedPtyAction::Submit, OrderedPtyAction::Raw(vec![0x03])],
        |text, enter| {
            assert!(text.is_empty());
            assert!(enter);
            written.borrow_mut().extend_from_slice(b"\r");
            Ok::<_, DeliveryError>(())
        },
        |bytes| {
            written.borrow_mut().extend_from_slice(bytes);
            Ok(())
        },
        |_| {},
    );

    result.unwrap();
    assert_eq!(written.into_inner(), b"\r\x03");
}

#[test]
fn a_keystroke_only_send_carries_no_return_address() {
    use super::prefixes_return_address;
    assert!(!prefixes_return_address("", true));
    assert!(prefixes_return_address("answer the prompt", true));
    assert!(!prefixes_return_address("answer the prompt", false));
}
