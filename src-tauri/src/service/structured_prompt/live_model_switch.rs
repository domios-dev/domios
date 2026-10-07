use std::time::Duration;

use serde_json::{json, Value};

const MODEL_SWITCH_PATH: &str = "/api/session/{sessionID}/model";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelId {
    pub provider_id: String,
    pub model_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LiveModelSwitchResult {
    Unsupported {
        reason: String,
    },
    Applied {
        observed_model: Option<String>,
        confirmation_detail: Option<String>,
    },
}

pub(crate) fn parse_model_id(value: &str) -> Option<ModelId> {
    let (provider_id, model_id) = value.trim().split_once('/')?;
    let provider_id = provider_id.trim();
    let model_id = model_id.trim();
    if provider_id.is_empty() || model_id.is_empty() {
        return None;
    }
    Some(ModelId {
        provider_id: provider_id.to_string(),
        model_id: model_id.to_string(),
    })
}

pub(crate) fn model_switch_route_available(document: &Value) -> bool {
    document
        .get("paths")
        .and_then(Value::as_object)
        .is_some_and(|paths| {
            paths.iter().any(|(path, operations)| {
                let path_matches =
                    path == MODEL_SWITCH_PATH || path == "/api/session/:sessionID/model";
                path_matches
                    && operations
                        .as_object()
                        .is_some_and(|operations| operations.contains_key("post"))
            })
        })
}

pub(crate) fn model_switch_body(model: &ModelId, effort: Option<&str>) -> Value {
    let mut body = json!({
        "model": {
            "id": model.model_id.clone(),
            "providerID": model.provider_id.clone(),
        }
    });
    if let Some(effort) = effort {
        body["model"]["variant"] = json!(effort);
    }
    body
}

pub(crate) fn parse_session_model(session_id: &str, document: &Value) -> Option<String> {
    if document.get("id")?.as_str()? != session_id {
        return None;
    }
    let model = document.get("model")?.as_object()?;
    let provider_id = model.get("providerID")?.as_str()?;
    let model_id = model.get("id").or_else(|| model.get("modelID"))?.as_str()?;
    (!provider_id.is_empty() && !model_id.is_empty()).then(|| format!("{provider_id}/{model_id}"))
}

pub(crate) fn same_model_id(left: &str, right: &str) -> bool {
    parse_model_id(left)
        .zip(parse_model_id(right))
        .is_some_and(|(left, right)| {
            left.provider_id.eq_ignore_ascii_case(&right.provider_id)
                && left.model_id.eq_ignore_ascii_case(&right.model_id)
        })
}

pub(crate) fn live_switch_for_tab(
    tab_id: &str,
    session_id: &str,
    requested_model: &str,
    effort: Option<&str>,
) -> Result<LiveModelSwitchResult, String> {
    let Some((base_url, cwd)) = super::endpoint_for_tab(tab_id) else {
        return Ok(LiveModelSwitchResult::Unsupported {
            reason: "the tab has no registered structured-prompt server".into(),
        });
    };
    let client = match reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return Ok(LiveModelSwitchResult::Unsupported {
                reason: format!("the tab server's /doc could not be probed: {error}"),
            })
        }
    };
    let doc_url = match endpoint_url(&base_url, &["doc"], None) {
        Ok(url) => url,
        Err(reason) => {
            return Ok(LiveModelSwitchResult::Unsupported {
                reason: format!("the tab server's /doc could not be addressed: {reason}"),
            })
        }
    };
    let document_response = match client.get(doc_url).send() {
        Ok(response) if response.status().is_success() => response.json::<Value>(),
        Ok(response) => {
            return Ok(LiveModelSwitchResult::Unsupported {
                reason: format!("the tab server's /doc returned HTTP {}", response.status()),
            })
        }
        Err(error) => {
            return Ok(LiveModelSwitchResult::Unsupported {
                reason: format!("the tab server's /doc could not be read: {error}"),
            })
        }
    };
    let document = match document_response {
        Ok(document) => document,
        Err(error) => {
            return Ok(LiveModelSwitchResult::Unsupported {
                reason: format!("the tab server's /doc was not valid OpenAPI JSON: {error}"),
            });
        }
    };
    if !model_switch_route_available(&document) {
        return Ok(LiveModelSwitchResult::Unsupported {
            reason: "the tab server's /doc does not advertise POST /api/session/{sessionID}/model"
                .into(),
        });
    }
    let Some(model) = parse_model_id(requested_model) else {
        return Ok(LiveModelSwitchResult::Unsupported {
            reason:
                "the requested model is not in provider/model form for the advertised live route"
                    .into(),
        });
    };

    let switch_url = endpoint_url(
        &base_url,
        &["api", "session", session_id, "model"],
        Some(&cwd),
    )?;
    let response = client
        .post(switch_url)
        .json(&model_switch_body(&model, effort))
        .send()
        .map_err(|error| format!("live model switch request failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "live model switch request returned HTTP {}",
            response.status()
        ));
    }

    let session_url = endpoint_url(&base_url, &["session", session_id], Some(&cwd))?;
    match client.get(session_url).send() {
        Ok(response) if response.status().is_success() => match response.json::<Value>() {
            Ok(session) => {
                let mut observed_model = parse_session_model(session_id, &session);
                let variant_matches = effort.is_none_or(|effort| {
                    session
                        .pointer("/model/variant")
                        .and_then(Value::as_str)
                        .unwrap_or("default")
                        == effort
                });
                if !variant_matches {
                    observed_model = None;
                }
                let confirmation_detail = match observed_model.as_deref() {
                    Some(observed) if same_model_id(requested_model, observed) => None,
                    Some(observed) => Some(format!(
                        "the bound session still reports {observed} after the live switch request"
                    )),
                    None => Some("the bound session record has not confirmed the requested model and variant".into()),
                };
                Ok(LiveModelSwitchResult::Applied {
                    observed_model,
                    confirmation_detail,
                })
            }
            Err(error) => Ok(LiveModelSwitchResult::Applied {
                observed_model: None,
                confirmation_detail: Some(format!(
                    "the bound session response was not valid JSON: {error}"
                )),
            }),
        },
        Ok(response) => Ok(LiveModelSwitchResult::Applied {
            observed_model: None,
            confirmation_detail: Some(format!(
                "the bound session GET returned HTTP {}",
                response.status()
            )),
        }),
        Err(error) => Ok(LiveModelSwitchResult::Applied {
            observed_model: None,
            confirmation_detail: Some(format!("the bound session GET failed: {error}")),
        }),
    }
}

fn endpoint_url(
    base_url: &str,
    path_segments: &[&str],
    directory: Option<&str>,
) -> Result<reqwest::Url, String> {
    let mut url = reqwest::Url::parse(base_url)
        .map_err(|error| format!("invalid structured-prompt endpoint: {error}"))?;
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|()| "structured-prompt endpoint cannot accept path segments".to_string())?;
        segments.clear();
        segments.extend(path_segments.iter().copied());
    }
    if let Some(directory) = directory {
        url.query_pairs_mut().append_pair("directory", directory);
    }
    Ok(url)
}

#[cfg(test)]
mod tests;
