//! Whether any surface will actually show one notification.
//!
//! Every surface a record can reach answers for itself elsewhere: the bell is
//! rendered or hidden by `show_bell`, the desktop toast is admitted by
//! `desktop::admit`, and push routing resolves channels for the declared level.
//! This is the one place that asks all three at once, so a caller can be told
//! the honest thing — that it wrote to a log nobody is watching.
//!
//! It reports only when EVERY surface is shut. A level whose interrupting
//! surfaces are closed while the bell still shows is the shape a fresh install
//! ships with; naming that would fire on the ordinary case and teach every
//! agent to ignore the line.

use serde::Serialize;

use super::desktop::{self, DesktopAdmission, DesktopPolicy};
use codeterm_types::notification::NotifyLevel;

/// The bell, the desktop toast, or push. Named on the wire so a caller reads a
/// reason per surface rather than one flattened sentence.
pub const BELL: &str = "bell";
pub const DESKTOP: &str = "desktop";
pub const PUSH: &str = "push";

/// One surface that will not show this notification, and why it will not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClosedSurface {
    pub surface: &'static str,
    pub reason: String,
}

/// Every surface is shut. Constructed only by [`unseen`], so nothing can claim
/// this without having asked all three.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Unseen {
    pub closed: Vec<ClosedSurface>,
}

/// What push routing resolved for this notification: how many channels will
/// actually carry it, and which routed channels refused.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PushReach {
    pub carrying: usize,
    pub refused: Vec<String>,
}

/// The facts that decide whether anything surfaces one notification. Pure: the
/// caller reads config and the push plan, this only judges them.
pub struct Reach<'a> {
    pub show_bell: bool,
    pub declared: Option<NotifyLevel>,
    pub desktop: &'a DesktopPolicy,
    pub push: PushReach,
}

fn unlevelled() -> String {
    "this notification names no level, so it can only be filed".to_string()
}

fn desktop_reason(admission: DesktopAdmission) -> Option<String> {
    match admission {
        DesktopAdmission::Allowed(_) => None,
        DesktopAdmission::Unlevelled => Some(unlevelled()),
        DesktopAdmission::DesktopOff => {
            Some("desktop notifications are off in settings".to_string())
        }
        DesktopAdmission::LevelDisabled(level) => Some(format!(
            "the {} level is not routed to the desktop",
            level.as_str()
        )),
    }
}

fn push_reason(declared: Option<NotifyLevel>, push: &PushReach) -> Option<String> {
    if push.carrying > 0 {
        return None;
    }
    if !push.refused.is_empty() {
        return Some(format!(
            "no push channel could carry it ({} refused)",
            push.refused.join(", ")
        ));
    }
    Some(match declared {
        None => unlevelled(),
        Some(level) => format!("no push channel is routed for the {} level", level.as_str()),
    })
}

/// `Some` only when the bell is hidden AND no toast is admitted AND no push
/// channel will carry it — the record exists and nothing will surface it.
pub fn unseen(reach: Reach<'_>) -> Option<Unseen> {
    if reach.show_bell {
        return None;
    }
    let desktop_shut = desktop_reason(desktop::admit(reach.declared, reach.desktop))?;
    let push_shut = push_reason(reach.declared, &reach.push)?;
    Some(Unseen {
        closed: vec![
            ClosedSurface {
                surface: BELL,
                reason: "the bell indicator is hidden (notifications.show_bell = false)"
                    .to_string(),
            },
            ClosedSurface {
                surface: DESKTOP,
                reason: desktop_shut,
            },
            ClosedSurface {
                surface: PUSH,
                reason: push_shut,
            },
        ],
    })
}

#[cfg(test)]
mod tests;
