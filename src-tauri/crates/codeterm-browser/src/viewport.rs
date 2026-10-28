pub const MIN_WIDTH: u32 = 320;
pub const MIN_HEIGHT: u32 = 240;
pub const MAX_WIDTH: u32 = 7680;
pub const MAX_HEIGHT: u32 = 4320;
pub const MIN_DPR: f64 = 1.0;
pub const MAX_DPR: f64 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
    pub dpr: f64,
    pub mobile: bool,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            width: 1600,
            height: 900,
            dpr: 1.0,
            mobile: false,
        }
    }
}

impl Viewport {
    /// Screencast frames are requested in device pixels so HiDPI viewers stay sharp.
    pub fn device_size(&self) -> (u32, u32) {
        (
            (self.width as f64 * self.dpr).round() as u32,
            (self.height as f64 * self.dpr).round() as u32,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewportRequest {
    pub width: f64,
    pub height: f64,
    pub dpr: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewportMode {
    Owned,
    Attached,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ViewportDecision {
    Resize(Viewport),
    Unchanged,
    Invalid,
    RefusedAttached,
}

/// CSS size and DPR clamp; `None` for a non-finite or non-positive size.
pub fn clamp(request: ViewportRequest) -> Option<Viewport> {
    let valid = |value: f64| value.is_finite() && value > 0.0;
    if !valid(request.width) || !valid(request.height) {
        return None;
    }
    let dpr = if request.dpr.is_finite() && request.dpr > 0.0 {
        request.dpr.clamp(MIN_DPR, MAX_DPR)
    } else {
        MIN_DPR
    };
    Some(Viewport {
        width: (request.width.round() as u32).clamp(MIN_WIDTH, MAX_WIDTH),
        height: (request.height.round() as u32).clamp(MIN_HEIGHT, MAX_HEIGHT),
        dpr,
        mobile: false,
    })
}

#[derive(Clone, Debug)]
pub struct ViewportModel {
    mode: ViewportMode,
    current: Viewport,
}

impl ViewportModel {
    pub fn new(mode: ViewportMode, initial: Viewport) -> Self {
        Self {
            mode,
            current: initial,
        }
    }

    pub fn mode(&self) -> ViewportMode {
        self.mode
    }

    pub fn current(&self) -> Viewport {
        self.current
    }

    /// An attached app owns its own window, so viewer sizes never resize or emulate it.
    pub fn apply(&mut self, request: ViewportRequest) -> ViewportDecision {
        self.apply_device(request, self.current.mobile)
    }

    pub fn apply_device(&mut self, request: ViewportRequest, mobile: bool) -> ViewportDecision {
        if self.mode == ViewportMode::Attached {
            return ViewportDecision::RefusedAttached;
        }
        let Some(mut next) = clamp(request) else {
            return ViewportDecision::Invalid;
        };
        next.mobile = mobile;
        if next == self.current {
            return ViewportDecision::Unchanged;
        }
        self.current = next;
        ViewportDecision::Resize(next)
    }
}

#[derive(Default)]
pub(crate) struct ResizeModel {
    applied: Option<Viewport>,
    in_flight: Option<Viewport>,
}

impl ResizeModel {
    pub fn begin(&mut self, latest: Viewport) -> Option<Viewport> {
        if self.in_flight.is_some() || self.applied == Some(latest) {
            return None;
        }
        self.in_flight = Some(latest);
        Some(latest)
    }

    pub fn acknowledge(&mut self, success: bool) {
        let finished = self.in_flight.take();
        self.applied = if success { finished } else { None };
    }
}

#[cfg(test)]
mod tests;
