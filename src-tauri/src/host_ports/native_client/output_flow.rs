use super::{Frame, NativeHostClient, NativeHostClientSlot};
use codeterm_types::native_host::NativeOutputFlow;
use tauri::Manager;

#[derive(Default)]
pub(super) struct OutputWindowState {
    active: bool,
    release_pending: bool,
}

impl OutputWindowState {
    fn before_write(&mut self, rendered: bool) -> (bool, bool) {
        let release = std::mem::take(&mut self.release_pending);
        (release, !rendered || self.active)
    }
    fn send(
        &mut self,
        update: NativeOutputFlow,
        mut send: impl FnMut(NativeOutputFlow) -> Result<(), String>,
    ) -> Result<(), String> {
        let release = matches!(update, NativeOutputFlow::DetachAll);
        self.release_pending |= release;
        if self.release_pending {
            send(NativeOutputFlow::DetachAll)?;
            self.release_pending = false;
        }
        if release {
            Ok(())
        } else {
            send(update)
        }
    }
}

impl NativeHostClient {
    pub(super) fn prepare_output_frames(&self, frame: Frame) -> Vec<Frame> {
        let Some(slot) = self.app.try_state::<NativeHostClientSlot>() else {
            return vec![frame];
        };
        let mut state = slot.1.lock();
        if slot
            .0
            .read()
            .as_ref()
            .is_none_or(|current| !std::ptr::eq(std::sync::Arc::as_ptr(current), self))
        {
            return Vec::new();
        }
        let rendered = matches!(
            &frame,
            Frame::OutputFlow {
                update: NativeOutputFlow::Rendered { .. },
                ..
            }
        );
        let (release, send) = state.before_write(rendered);
        let mut frames = Vec::with_capacity(2);
        if release {
            if let Some(view) = self.view.lock().as_ref() {
                frames.push(Frame::OutputFlow {
                    connection: self.connection.clone(),
                    view_session: view.session_id,
                    update: NativeOutputFlow::DetachAll,
                });
            }
        }
        if send {
            frames.push(frame);
        }
        frames
    }
}

impl NativeHostClientSlot {
    pub(crate) fn report_output_flow(
        &self,
        origin: Option<&str>,
        update: NativeOutputFlow,
    ) -> Result<bool, String> {
        let mut state = self.1.lock();
        let active = state.active;
        let update = if active {
            update
        } else {
            NativeOutputFlow::DetachAll
        };
        // The view origin is bound by the native navigation observer, not by the renderer.
        state.send(update, |update| self.send_output_flow(origin, update))?;
        Ok(active)
    }

    pub(crate) fn set_output_window_active(&self, active: bool) {
        if cfg!(windows) {
            return;
        }
        let mut state = self.1.lock();
        state.active = active;
        if !active {
            let _ = state.send(NativeOutputFlow::DetachAll, |update| {
                self.send_output_flow(None, update)
            });
        }
    }
}

#[cfg(test)]
mod tests;
