use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum PaneStatus {
    Working,
    Waiting,
    Idle,
    Dead,
}
