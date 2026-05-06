use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::agent::provider::ModelInfo;

const MODEL_CACHE_TTL: Duration = Duration::from_secs(300);

struct ModelCache {
    fetched_at: Instant,
    models: Vec<ModelInfo>,
}

fn model_cache() -> &'static Mutex<Option<ModelCache>> {
    static CACHE: OnceLock<Mutex<Option<ModelCache>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

pub(super) fn cached_opencode_models() -> Option<Vec<ModelInfo>> {
    let guard = model_cache().lock();
    let entry = guard.as_ref()?;
    if entry.fetched_at.elapsed() < MODEL_CACHE_TTL {
        Some(entry.models.clone())
    } else {
        None
    }
}

pub(super) fn store_opencode_models(models: Vec<ModelInfo>) {
    *model_cache().lock() = Some(ModelCache {
        fetched_at: Instant::now(),
        models,
    });
}

pub(super) fn fetch_opencode_models() -> Result<Vec<ModelInfo>, String> {
    let mut cmd = Command::new("opencode");
    cmd.arg("models");
    crate::process_util::no_window_std(&mut cmd);
    let output = cmd
        .output()
        .map_err(|e| format!("spawn opencode: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "opencode models exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let models: Vec<ModelInfo> = stdout
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && l.contains('/'))
        .map(|id| ModelInfo {
            id: id.to_string(),
            display_name: prettify_model_id(id),
            description: None,
        })
        .collect();

    Ok(models)
}

pub(super) fn prettify_model_id(id: &str) -> String {
    let (provider, model) = id.split_once('/').unwrap_or(("", id));
    let provider_label = match provider {
        "anthropic" => "Anthropic",
        "openai" => "OpenAI",
        "google" => "Google",
        "github-copilot" => "GitHub Copilot",
        "openrouter" => "OpenRouter",
        "opencode" => "OpenCode",
        other if !other.is_empty() => other,
        _ => "",
    };
    let model_label = model
        .split('-')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    if provider_label.is_empty() {
        model_label
    } else {
        format!("{} · {}", provider_label, model_label)
    }
}

pub(super) fn fallback_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "anthropic/claude-opus-4-7".into(),
            display_name: "Anthropic · Claude Opus 4.7".into(),
            description: Some("Anthropic frontier model — strongest coding intelligence.".into()),
        },
        ModelInfo {
            id: "anthropic/claude-sonnet-4-6".into(),
            display_name: "Anthropic · Claude Sonnet 4.6".into(),
            description: Some("Anthropic balanced model — fast + capable.".into()),
        },
        ModelInfo {
            id: "anthropic/claude-haiku-4-5".into(),
            display_name: "Anthropic · Claude Haiku 4.5".into(),
            description: Some("Anthropic small model — cheap, fast.".into()),
        },
        ModelInfo {
            id: "openai/gpt-5".into(),
            display_name: "OpenAI · GPT-5".into(),
            description: Some("OpenAI frontier model.".into()),
        },
        ModelInfo {
            id: "opencode/big-pickle".into(),
            display_name: "OpenCode · Big Pickle".into(),
            description: Some("OpenCode-hosted frontier coding model.".into()),
        },
    ]
}
