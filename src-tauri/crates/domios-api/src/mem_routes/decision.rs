//! Bearer-only supporter token, connection and status routes.

use std::io;
use std::sync::Arc;

use axum::{
    extract::{Query, State as AxumState},
    http::StatusCode,
    Json,
};
use codeterm_decision::TokenMode;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::routes::AppState;
use codeterm_service_core::config::schema::SupporterConfig;
use codeterm_service_core::mem::decision::token_store::{TokenStatus, TokenStore};
use codeterm_service_core::service::supporter::registry::{
    registered_adapters, supporter_state, token_mode_name, token_store, SupporterRegistry,
};

mod supporter_json;
pub use supporter_json::supporter_error_envelope;

mod connection_routes;
pub use connection_routes::*;

mod supporter_routes;
pub use supporter_routes::*;
