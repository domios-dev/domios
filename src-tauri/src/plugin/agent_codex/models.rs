use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Deserialize;

use crate::agent::provider::ModelInfo;

const MODEL_CACHE_TTL: Duration = Duration::from_secs(300);
const FALLBACK_DEFAULT_MODEL: &str = "gpt-5.5";

struct ModelCache {
    fetched_at: Instant,
    models: Vec<ModelInfo>,
}

fn model_cache() -> &'static Mutex<Option<ModelCache>> {
    static CACHE: OnceLock<Mutex<Option<ModelCache>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

pub(super) fn available_codex_models() -> Vec<ModelInfo> {
    if let Some(o) = crate::agent::model_catalog::try_override("codex") {
        return o;
    }
    if let Some(cached) = cached_codex_models() {
        return cached;
    }
    match fetch_codex_models() {
        Ok(list) if !list.is_empty() => {
            store_codex_models(list.clone());
            list
        }
        _ => fallback_models(),
    }
}

pub(super) fn default_codex_model() -> Option<String> {
    crate::agent::model_catalog::try_override_default("codex").or_else(|| {
        let models = available_codex_models();
        if models.iter().any(|m| m.id == FALLBACK_DEFAULT_MODEL) {
            Some(FALLBACK_DEFAULT_MODEL.to_string())
        } else {
            models.first().map(|m| m.id.clone())
        }
    })
}

fn cached_codex_models() -> Option<Vec<ModelInfo>> {
    let guard = model_cache().lock();
    let entry = guard.as_ref()?;
    if entry.fetched_at.elapsed() < MODEL_CACHE_TTL {
        Some(entry.models.clone())
    } else {
        None
    }
}

fn store_codex_models(models: Vec<ModelInfo>) {
    *model_cache().lock() = Some(ModelCache {
        fetched_at: Instant::now(),
        models,
    });
}

fn fetch_codex_models() -> Result<Vec<ModelInfo>, String> {
    let mut cmd = Command::new("codex");
    cmd.args(["debug", "models"]);
    crate::process_util::no_window_std(&mut cmd);
    let output = cmd.output().map_err(|e| format!("spawn codex: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "codex debug models exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    parse_codex_models_json(&output.stdout)
}

#[derive(Deserialize)]
struct CodexModelCatalog {
    models: Vec<CodexModel>,
}

#[derive(Deserialize)]
struct CodexModel {
    slug: String,
    display_name: Option<String>,
    description: Option<String>,
    visibility: Option<String>,
    priority: Option<i64>,
}

fn parse_codex_models_json(bytes: &[u8]) -> Result<Vec<ModelInfo>, String> {
    let mut entries: Vec<CodexModel> = serde_json::from_slice::<CodexModelCatalog>(bytes)
        .map_err(|e| format!("parse codex model catalog: {}", e))?
        .models
        .into_iter()
        .filter(|m| {
            !m.slug.trim().is_empty() && m.visibility.as_deref().unwrap_or("list") == "list"
        })
        .collect();

    entries.sort_by_key(|m| m.priority.unwrap_or(i64::MAX));

    Ok(entries
        .into_iter()
        .map(|m| {
            let display_name = m
                .display_name
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| prettify_model_id(&m.slug));
            ModelInfo {
                id: m.slug,
                display_name,
                description: m.description,
            }
        })
        .collect())
}

fn prettify_model_id(id: &str) -> String {
    id.split('-')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("-")
}

pub(super) fn fallback_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "gpt-5.5".into(),
            display_name: "GPT-5.5".into(),
            description: Some(
                "Frontier model for complex coding, research, and real-world work.".into(),
            ),
        },
        ModelInfo {
            id: "gpt-5.4".into(),
            display_name: "GPT-5.4".into(),
            description: Some("Strong model for everyday coding.".into()),
        },
        ModelInfo {
            id: "gpt-5.4-mini".into(),
            display_name: "GPT-5.4-Mini".into(),
            description: Some(
                "Small, fast, and cost-efficient model for simpler coding tasks.".into(),
            ),
        },
        ModelInfo {
            id: "gpt-5.3-codex-spark".into(),
            display_name: "GPT-5.3-Codex-Spark".into(),
            description: Some("Ultra-fast coding model.".into()),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_codex_debug_model_catalog() {
        let json = br#"{
          "models": [
            {
              "slug": "hidden-review",
              "display_name": "Hidden Review",
              "description": "Internal",
              "visibility": "hide",
              "priority": 1
            },
            {
              "slug": "gpt-5.4-mini",
              "display_name": "GPT-5.4-Mini",
              "description": "Small, fast",
              "visibility": "list",
              "priority": 20
            },
            {
              "slug": "gpt-5.5",
              "display_name": "GPT-5.5",
              "description": "Frontier",
              "visibility": "list",
              "priority": 10
            }
          ]
        }"#;

        let models = parse_codex_models_json(json).expect("parse");
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "gpt-5.5");
        assert_eq!(models[0].display_name, "GPT-5.5");
        assert_eq!(models[0].description.as_deref(), Some("Frontier"));
        assert_eq!(models[1].id, "gpt-5.4-mini");
    }
}
