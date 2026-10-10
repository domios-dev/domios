use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

pub fn websocket_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(64 * 1024 * 1024))
        .max_frame_size(Some(64 * 1024 * 1024))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tokio_tungstenite::tungstenite::{protocol::Role, Error, Message, WebSocket};

    fn header(length: u64) -> Vec<u8> {
        let mut frame = vec![0x81, 127];
        frame.extend_from_slice(&length.to_be_bytes());
        frame
    }

    #[test]
    fn codex_rpc_attach_websocket_accepts_measured_plugin_catalogue_frame() {
        assert_text_frame(12_389_104);
    }

    #[test]
    fn codex_rpc_attach_websocket_accepts_single_frame_above_former_frame_limit() {
        assert_text_frame(16 * 1024 * 1024 + 1);
    }

    fn assert_text_frame(length: usize) {
        let mut frame = header(length as u64);
        frame.resize(frame.len() + length, b'x');
        let mut socket =
            WebSocket::from_raw_socket(Cursor::new(frame), Role::Client, Some(websocket_config()));
        let Message::Text(text) = socket.read().unwrap() else {
            panic!("expected text frame")
        };
        assert_eq!(text.len(), length);
    }

    #[test]
    fn codex_rpc_attach_websocket_retains_bounded_frame_memory() {
        let frame = header(64 * 1024 * 1024 + 1);
        let mut socket =
            WebSocket::from_raw_socket(Cursor::new(frame), Role::Client, Some(websocket_config()));
        assert!(matches!(socket.read(), Err(Error::Capacity(_))));
    }
}
