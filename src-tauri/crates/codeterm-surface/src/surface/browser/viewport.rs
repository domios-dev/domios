use codeterm_browser::viewport::{clamp, Viewport, ViewportRequest};

use super::BrowserRuntime;

pub(super) fn recursive_viewer(viewer_target: Option<&str>, page_target: Option<&str>) -> bool {
    viewer_target.is_some() && viewer_target == page_target
}

#[derive(Default)]
pub(super) struct ViewportOwner {
    viewers: Vec<(String, u64)>,
    owner: Option<String>,
    fixed: Option<Viewport>,
    generation: u64,
    emitted: Option<Viewport>,
}

impl ViewportOwner {
    pub fn join(
        &mut self,
        viewer: String,
        viewport: ViewportRequest,
        recursive: bool,
    ) -> Option<u64> {
        if recursive {
            return None;
        }
        let viewport = clamp(viewport)?;
        self.generation += 1;
        self.viewers.retain(|(id, ..)| id != &viewer);
        self.viewers.push((viewer.clone(), self.generation));
        if self.fixed.is_none() {
            self.owner = Some(viewer);
            self.fixed = Some(viewport);
        }
        Some(self.generation)
    }

    pub fn owner(&self) -> Option<String> {
        self.owner.clone()
    }

    pub fn current(&self) -> Option<Viewport> {
        self.fixed
    }

    pub fn next_resize(&mut self) -> Option<Viewport> {
        let next = self.current()?;
        if self.emitted == Some(next) {
            return None;
        }
        self.emitted = Some(next);
        Some(next)
    }

    pub fn request(
        &mut self,
        viewer: &str,
        request: ViewportRequest,
        claim: bool,
    ) -> Option<Viewport> {
        if !claim || !self.viewers.iter().any(|(id, _)| id == viewer) {
            return None;
        }
        let viewport = clamp(request)?;
        self.fixed = Some(viewport);
        self.owner = Some(viewer.to_owned());
        Some(viewport)
    }

    pub fn leave(&mut self, viewer: &str, generation: u64) -> bool {
        let before = self.viewers.len();
        self.viewers
            .retain(|(id, lease)| id != viewer || *lease != generation);
        before != self.viewers.len()
    }

    pub fn agent(&mut self, viewport: Viewport) {
        self.fixed = Some(viewport);
        self.emitted = Some(viewport);
        self.owner = None;
    }

    pub fn start(&mut self, viewport: Viewport) {
        if self.fixed.is_none() {
            self.agent(viewport);
        }
    }
}

impl BrowserRuntime {
    pub(super) async fn apply_viewer_viewport(
        &self,
        model: &mut ViewportOwner,
    ) -> Result<(), String> {
        self.shared
            .update(|state| state.viewport_owner = model.owner());
        if let Some(viewport) = model.next_resize() {
            log::debug!(
                "[browser-viewport] apply css={}x{} dpr={} owner={:?}",
                viewport.width,
                viewport.height,
                viewport.dpr,
                model.owner()
            );
            if let Err(error) = self
                .host
                .set_viewport(ViewportRequest {
                    width: viewport.width as f64,
                    height: viewport.height as f64,
                    dpr: viewport.dpr,
                })
                .await
            {
                model.emitted = None;
                return Err(error);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
