use codeterm_browser::viewport::{clamp, Viewport, ViewportRequest};

use super::BrowserRuntime;

pub(super) fn recursive_viewer(viewer_target: Option<&str>, page_target: Option<&str>) -> bool {
    viewer_target.is_some() && viewer_target == page_target
}

#[derive(Default)]
pub(super) struct ViewportOwner {
    viewers: Vec<(String, u64, Viewport)>,
    owner: Option<String>,
    agent: bool,
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
        self.viewers
            .push((viewer.clone(), self.generation, viewport));
        if self.owner.is_none() && !self.agent {
            self.owner = Some(viewer);
        }
        Some(self.generation)
    }

    pub fn owner(&self) -> Option<String> {
        self.owner.clone()
    }

    pub fn current(&self) -> Option<Viewport> {
        self.viewers
            .iter()
            .find(|(id, ..)| Some(id) == self.owner.as_ref())
            .map(|(_, _, viewport)| *viewport)
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
        let viewport = clamp(request)?;
        let entry = self.viewers.iter_mut().find(|(id, ..)| id == viewer)?;
        entry.2 = viewport;
        if claim {
            self.agent = false;
            self.owner = Some(viewer.to_owned());
        }
        (self.owner.as_deref() == Some(viewer)).then_some(viewport)
    }

    pub fn leave(&mut self, viewer: &str, generation: u64) -> bool {
        let before = self.viewers.len();
        self.viewers
            .retain(|(id, lease, _)| id != viewer || *lease != generation);
        if before == self.viewers.len() {
            return false;
        }
        if self.owner.as_deref() == Some(viewer) {
            self.owner = self.viewers.last().map(|(id, ..)| id.clone());
            return true;
        }
        false
    }

    pub fn agent(&mut self) {
        self.emitted = None;
        self.agent = true;
        self.owner = None;
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
