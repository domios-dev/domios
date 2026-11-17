use super::BrowserRuntime;
use crate::surface::runtime::SurfaceNotice;

impl BrowserRuntime {
    pub(super) async fn switch_signin(&self, visible: bool) -> Result<(), String> {
        let _guard = self.action_gate.lock().await;
        self.shared.invalidate_action();
        self.shared.invalidate_snapshot();
        self.host.set_visible(visible).await?;
        self.shared.unbind();
        if visible {
            self.shared.notice(SurfaceNotice::info(
                "Sign in in the Chromium window on this machine. When finished, choose Continue headless. The Browser preview is paused.",
            ));
        } else {
            self.shared.notice(SurfaceNotice::info(
                "Continuing headless with the same profile",
            ));
        }
        Ok(())
    }
}
