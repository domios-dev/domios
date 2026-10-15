use crate::service::ServiceContext;

const WINDOWS_CTRL_C: &[u8] = b"\x1b[67;46;99;1;8;1_\x1b[67;46;99;0;8;1_";

fn encode_control_key(frame: &[u8], windows: bool, declared: bool) -> Option<&'static [u8]> {
    // ConPTY needs the character and Ctrl modifier; Unicode 3 loses the chord.
    (windows && declared && frame == b"\x03").then_some(WINDOWS_CTRL_C)
}

impl ServiceContext {
    pub fn encode_provider_control_key(&self, pty_id: &str, frame: &[u8]) -> Option<&'static [u8]> {
        if !cfg!(windows) || frame != b"\x03" {
            return None;
        }
        let provider_id = {
            let mux = self.mux.read();
            let tab_id = mux.tab_id_for_pty(pty_id)?;
            mux.get_tab(&tab_id)?.agent_type.clone()?
        };
        let declared = self
            .agent_registry
            .with_provider(&provider_id, |provider| provider.uses_win32_control_keys())
            .unwrap_or(false);
        encode_control_key(frame, cfg!(windows), declared)
    }
}

#[cfg(test)]
mod tests {
    use super::encode_control_key;

    #[test]
    fn declared_windows_ctrl_c_is_one_key_down_and_key_up() {
        assert_eq!(
            encode_control_key(b"\x03", true, true),
            Some(b"\x1b[67;46;99;1;8;1_\x1b[67;46;99;0;8;1_".as_slice())
        );
    }

    #[test]
    fn undeclared_windows_and_all_unix_inputs_remain_byte_exact() {
        for windows in [false, true] {
            for declared in [false, true] {
                for frame in [
                    b"\x1b".as_slice(),
                    b"text\x03text",
                    b"\x1b[200~\x03\x1b[201~",
                ] {
                    assert_eq!(encode_control_key(frame, windows, declared), None);
                }
                if !windows || !declared {
                    assert_eq!(encode_control_key(b"\x03", windows, declared), None);
                }
            }
        }
    }
}
