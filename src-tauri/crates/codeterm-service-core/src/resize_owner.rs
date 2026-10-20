pub use codeterm_pty::resize_owner::*;

use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct ResizeOwnerClaim {
    pub owner: ResizeOwner,
    pub current_size: Option<ResizePtySize>,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct ResizePtySize {
    pub cols: u16,
    pub rows: u16,
}

impl From<(u16, u16)> for ResizePtySize {
    fn from((cols, rows): (u16, u16)) -> Self {
        Self { cols, rows }
    }
}
