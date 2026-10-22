use crate::{PtyState, ScreenStateHandle};

pub fn resize(
    pty: &PtyState,
    screen_state: &ScreenStateHandle,
    pty_id: &str,
    requested: Option<(u16, u16)>,
) -> Result<(), String> {
    if requested.is_some_and(|(cols, rows)| cols == 0 || rows == 0) {
        return Err("PTY dimensions must be nonzero".into());
    }
    let session = pty
        .lock()
        .get_session(pty_id)
        .ok_or_else(|| format!("PTY not found: {pty_id}"))?;
    // Serialize geometry and output parsing before the PTY can emit its resize repaint.
    let screens = screen_state.pty(pty_id);
    let mut screens = screens.lock_for_update();
    let previous = *session.size.lock().map_err(|e| e.to_string())?;
    let (cols, rows) = requested.unwrap_or(previous);
    let changed = session.resize(cols, rows)?;
    screens.resize(cols, rows);
    log::debug!(
        "[pty-resize] pty={pty_id} previous={}x{} applied={cols}x{rows} changed={changed}",
        previous.0,
        previous.1
    );
    if changed {
        crate::pty::capture::CaptureRegistry::global().tap_resize(pty_id, cols, rows);
    }
    Ok(())
}
