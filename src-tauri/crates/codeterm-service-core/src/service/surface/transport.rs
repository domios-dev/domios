use super::{SURFACE_NO_INPUT, SURFACE_NO_TEXT};
use crate::service::pty_io::SendBatchItem;
use crate::service::tab_transport::TabTransport;

/// Surface tabs have no terminal: text in is refused, text out is refused, resize and kill are no-ops.
pub(crate) struct SurfaceTransport;

pub(super) fn no_input() -> String {
    format!("{SURFACE_NO_INPUT}: surface tabs have no text input; drive the surface through its plugin's verbs")
}

fn no_text() -> String {
    format!("{SURFACE_NO_TEXT}: surface tabs render a plugin view and have no terminal text")
}

impl TabTransport for SurfaceTransport {
    fn send(&self, _text: &str, _enter: bool) -> Result<(), String> {
        Err(no_input())
    }

    fn send_batch(&self, _batch: Vec<SendBatchItem>) -> Result<(), String> {
        Err(no_input())
    }

    fn resize(&self, _cols: u16, _rows: u16) -> Result<(), String> {
        Ok(())
    }

    fn kill(&self) -> Result<(), String> {
        Ok(())
    }

    fn read_output(
        &self,
        _offset: Option<usize>,
        _max_bytes: Option<usize>,
        _strip_ansi: bool,
        _full: bool,
    ) -> Result<serde_json::Value, String> {
        Err(no_text())
    }

    fn read_screen(&self, _formatted: bool) -> Result<serde_json::Value, String> {
        Err(no_text())
    }
}
