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
    let output = cmd.output().map_err(|e| format!("spawn opencode: {}", e))?;

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
            id: "opencode/big-pickle".into(),
            display_name: "OpenCode · Big Pickle".into(),
            description: Some("OpenCode-hosted frontier coding model.".into()),
        },
        ModelInfo {
            id: "opencode/mimo-v2.5-free".into(),
            display_name: "OpenCode · Mimo V2.5 Free".into(),
            description: Some("Free tier coding model.".into()),
        },
        ModelInfo {
            id: "opencode/deepseek-v4-flash-free".into(),
            display_name: "OpenCode · DeepSeek V4 Flash Free".into(),
            description: Some("Free tier, fast DeepSeek model.".into()),
        },
        ModelInfo {
            id: "opencode/nemotron-3-ultra-free".into(),
            display_name: "OpenCode · Nemotron 3 Ultra Free".into(),
            description: Some("Free tier model.".into()),
        },
        ModelInfo {
            id: "opencode/claude-opus-4-8".into(),
            display_name: "OpenCode · Claude Opus 4.8".into(),
            description: Some("Claude Opus via OpenCode-hosted routing.".into()),
        },
        ModelInfo {
            id: "opencode/claude-sonnet-4-6".into(),
            display_name: "OpenCode · Claude Sonnet 4.6".into(),
            description: Some("Claude Sonnet via OpenCode-hosted routing.".into()),
        },
        ModelInfo {
            id: "opencode/gpt-5.5".into(),
            display_name: "OpenCode · GPT-5.5".into(),
            description: Some("GPT-5.5 via OpenCode-hosted routing.".into()),
        },
    ]
}
