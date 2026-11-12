use axum::{
    routing::{get, post},
    Router,
};

use super::routes::AppState;

mod exchange;
mod peer;
mod template_start;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/team-templates", get(template_start::list))
        .route("/api/team-templates/preview", post(template_start::preview))
        .route("/api/team-templates/peer-start", post(peer::peer_start))
        .route("/api/team-templates/peer-outcome", post(peer::peer_outcome))
        .route("/api/team-templates/transfer", post(peer::transfer))
        .route("/api/team-templates/recall", post(exchange::recall))
        .route("/api/team-templates/export", post(exchange::export))
        .route("/api/team-templates/import", post(exchange::import))
        .route("/api/team-templates/delete", post(exchange::delete))
        .route("/api/team-templates/card", post(exchange::card))
        .route("/api/teams/{team_id}/distill", post(exchange::distill))
}
