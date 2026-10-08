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
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            width: 1600,
            height: 900,
            dpr: 1.0,
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
        if self.mode == ViewportMode::Attached {
            return ViewportDecision::RefusedAttached;
        }
        let Some(next) = clamp(request) else {
            return ViewportDecision::Invalid;
        };
        if next == self.current {
            return ViewportDecision::Unchanged;
        }
        self.current = next;
        ViewportDecision::Resize(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(width: f64, height: f64, dpr: f64) -> ViewportRequest {
        ViewportRequest { width, height, dpr }
    }

    #[test]
    fn browser_viewport_clamps_size_and_dpr() {
        assert_eq!(
            clamp(request(100.4, 50.0, 5.0)),
            Some(Viewport {
                width: MIN_WIDTH,
                height: MIN_HEIGHT,
                dpr: MAX_DPR
            })
        );
        assert_eq!(
            clamp(request(1280.6, 720.2, 1.5)),
            Some(Viewport {
                width: 1281,
                height: 720,
                dpr: 1.5
            })
        );
        assert_eq!(
            clamp(request(99999.0, 99999.0, 0.5)).unwrap().width,
            MAX_WIDTH
        );
        assert_eq!(clamp(request(800.0, 600.0, 0.5)).unwrap().dpr, MIN_DPR);
        assert_eq!(clamp(request(800.0, 600.0, f64::NAN)).unwrap().dpr, MIN_DPR);
    }

    #[test]
    fn browser_viewport_rejects_non_positive_or_non_finite_size() {
        assert_eq!(clamp(request(0.0, 600.0, 1.0)), None);
        assert_eq!(clamp(request(800.0, -1.0, 1.0)), None);
        assert_eq!(clamp(request(f64::INFINITY, 600.0, 1.0)), None);
        assert_eq!(clamp(request(f64::NAN, 600.0, 1.0)), None);
    }

    #[test]
    fn browser_viewport_device_size_scales_by_dpr() {
        let viewport = Viewport {
            width: 1280,
            height: 721,
            dpr: 1.5,
        };
        assert_eq!(viewport.device_size(), (1920, 1082));
    }

    #[test]
    fn browser_viewport_owned_mode_resizes_once_per_change() {
        let mut model = ViewportModel::new(ViewportMode::Owned, Viewport::default());
        let first = model.apply(request(1280.0, 720.0, 2.0));
        assert!(matches!(first, ViewportDecision::Resize(v) if v.width == 1280 && v.dpr == 2.0));
        assert_eq!(
            model.apply(request(1280.0, 720.0, 2.0)),
            ViewportDecision::Unchanged
        );
        assert_eq!(
            model.apply(request(0.0, 0.0, 1.0)),
            ViewportDecision::Invalid
        );
        assert_eq!(model.current().width, 1280);
    }

    #[test]
    fn browser_viewport_attach_mode_never_resizes() {
        let initial = Viewport::default();
        let mut model = ViewportModel::new(ViewportMode::Attached, initial);
        assert_eq!(
            model.apply(request(800.0, 600.0, 1.0)),
            ViewportDecision::RefusedAttached
        );
        assert_eq!(model.current(), initial);
    }
}
