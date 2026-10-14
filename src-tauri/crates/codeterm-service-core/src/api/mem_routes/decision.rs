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

use crate::api::routes::AppState;
use crate::config::schema::SupporterConfig;
use crate::mem::decision::token_store::{TokenStatus, TokenStore};
use crate::service::supporter::registry::{
    registered_adapters, SupporterRegistry, supporter_state, token_mode_name, token_store,
};

mod supporter_json;
pub use supporter_json::supporter_error_envelope;

mod connection_routes;
pub use connection_routes::*;

mod supporter_routes;
pub use supporter_routes::*;
