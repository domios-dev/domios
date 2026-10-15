use axum::Router;

#[derive(Clone)]
pub struct LocalApiRouter(pub Router);
