//! Memory operations shared by the HTTP routes and the socket RPC surface.

pub(crate) mod area_merge;
pub(crate) mod calibration;
pub(crate) mod dream;
pub(crate) mod graph;
pub(crate) mod items;
pub(crate) mod mem_merge;
pub(crate) mod search;
pub(crate) mod sync;

use axum::http::StatusCode;

pub(crate) fn validate_item_fields(
    title: &str,
    body: &str,
    area: &Option<String>,
    tags: &[String],
    importance: Option<f64>,
) -> Result<(), (StatusCode, String)> {
    codeterm_mem::curation::validate_item_fields(title, body, area, tags, importance).map_err(
        |(code, msg)| {
            (
                StatusCode::from_u16(code).unwrap_or(StatusCode::BAD_REQUEST),
                msg,
            )
        },
    )
}

pub(crate) use codeterm_mem::curation::sanitize_actor;
