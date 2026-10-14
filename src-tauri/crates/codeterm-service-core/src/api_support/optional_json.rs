use axum::{
    body::Bytes,
    extract::{FromRequest, Request},
    response::{IntoResponse, Response},
    Json,
};
use serde::de::DeserializeOwned;

pub(crate) struct OptionalJson<T>(pub Option<T>);

impl<S, T> FromRequest<S> for OptionalJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let content_type = req.headers().get(axum::http::header::CONTENT_TYPE).cloned();
        let bytes = Bytes::from_request(req, state)
            .await
            .map_err(IntoResponse::into_response)?;
        if bytes.is_empty() {
            return Ok(Self(None));
        }
        let is_json = content_type
            .and_then(|value| value.to_str().ok().map(str::to_owned))
            .is_some_and(|value| {
                let mime = value.split(';').next().unwrap_or_default().trim();
                mime == "application/json"
                    || (mime.starts_with("application/") && mime.ends_with("+json"))
            });
        if !is_json {
            return Err((
                axum::http::StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Expected a JSON request body",
            )
                .into_response());
        }
        Json::from_bytes(&bytes)
            .map(|Json(value)| Self(Some(value)))
            .map_err(IntoResponse::into_response)
    }
}
