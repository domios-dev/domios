use codeterm_pty::pty::input_mode::encode_action_key;

#[test]
fn declared_windows_ctrl_c_is_one_key_down_and_key_up() {
    assert_eq!(
        encode_action_key("ctrl-c", true, true).unwrap(),
        b"\x1b[67;0;99;1;8;1_\x1b[67;0;99;0;8;1_"
    );
}

#[test]
fn undeclared_windows_and_all_unix_inputs_remain_byte_exact() {
    for (windows, win32) in [(false, false), (false, true), (true, false)] {
        assert_eq!(encode_action_key("ctrl-c", windows, win32), Some(vec![3]));
    }
}
