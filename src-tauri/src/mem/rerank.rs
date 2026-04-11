//! LLM rerank trait + Noop / Haiku impls. Provider-agnostic interface so
//! Codex / Sonnet / local model rerankers can plug in by implementing the
//! trait. Default is Noop (return input order); enabling rerank requires
//! `mem.rerank.enabled = true` in config + a working API key.

use serde::{Deserialize, Serialize};

use super::search::SearchHit;

pub trait Reranker: Send + Sync {
    fn rerank(&self, query: &str, hits: Vec<SearchHit>, top_k: usize) -> Vec<SearchHit>;
}

#[derive(Debug, Default)]
pub struct NoopReranker;

impl Reranker for NoopReranker {
    fn rerank(&self, _query: &str, mut hits: Vec<SearchHit>, top_k: usize) -> Vec<SearchHit> {
        hits.truncate(top_k);
        hits
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankConfig {
    pub enabled: bool,
    pub provider: String, // "noop" | "haiku" | "sonnet" | "codex"
    pub api_key_env: Option<String>,
    pub model: Option<String>,
}

impl Default for RerankConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: "auto".into(), // when enabled, picks claude-cli → codex-cli → API → noop
            api_key_env: None,
            model: None,
        }
    }
}

/// Build a Reranker from config. Falls back to Noop on misconfiguration.
///
/// Provider precedence ("auto" is the default and Just Works for most users):
/// - **"auto"** — tries the local fastembed cross-encoder first (truly free,
///   no network, ~100-300ms per rerank), then `claude --bare -p` if
///   ANTHROPIC_API_KEY is set, then OpenAI API, then Noop.
/// - **"local"** — local fastembed cross-encoder (BAAI/bge-reranker-base by
///   default; downloads ~280MB once on first use, then offline forever).
///   This is the default for "auto" because it's the only path that costs
///   nothing per query.
/// - **"claude-cli"** — `claude --bare --print --model haiku ...` subprocess.
///   Requires ANTHROPIC_API_KEY (--bare disables oauth/keychain). Without
///   --bare, claude loads CLAUDE.md + plugins + hooks → 42K input tokens
///   cached, ~$0.05/call, 2.5s latency. With --bare it's ~$0.001/call.
///   Use only if you specifically want a model rerank and have the key.
/// - **"codex-cli"** — `codex exec --model gpt-5-mini ...` subprocess.
///   Same trade-off; requires the user's codex login to be active.
/// - **"haiku" | "sonnet" | "anthropic"** — direct Anthropic API
///   (needs ANTHROPIC_API_KEY).
/// - **"openai" | "gpt"** — direct OpenAI API (needs OPENAI_API_KEY).
/// - anything else → Noop
pub fn build(config: &RerankConfig) -> Box<dyn Reranker> {
    if !config.enabled {
        return Box::new(NoopReranker);
    }
    match config.provider.as_str() {
        "local" | "fastembed" | "bge" | "jina" => build_local(config).unwrap_or_else(|| {
            log::warn!("[mem.rerank] local fastembed reranker unavailable, falling back to Noop");
            Box::new(NoopReranker)
        }),
        "claude-cli" | "claude" => build_claude_cli(config).unwrap_or_else(|| {
            log::warn!("[mem.rerank] claude CLI not on PATH, falling back to Noop");
            Box::new(NoopReranker)
        }),
        "codex-cli" => build_codex_cli(config).unwrap_or_else(|| {
            log::warn!("[mem.rerank] codex CLI not on PATH, falling back to Noop");
            Box::new(NoopReranker)
        }),
        "haiku" | "sonnet" | "anthropic" => build_anthropic(config).unwrap_or_else(|| {
            log::warn!("[mem.rerank] anthropic unavailable, falling back to Noop");
            Box::new(NoopReranker)
        }),
        "codex" | "openai" | "gpt" => build_openai(config).unwrap_or_else(|| {
            log::warn!("[mem.rerank] openai unavailable, falling back to Noop");
            Box::new(NoopReranker)
        }),
        "auto" | "" => {
            // Local first: free, fast, offline. Then API paths if user opted in.
            if let Some(r) = build_local(config) { return r; }
            if let Some(r) = build_anthropic(config) { return r; }
            if let Some(r) = build_openai(config) { return r; }
            log::warn!("[mem.rerank] auto: no rerank backend available, falling back to Noop");
            Box::new(NoopReranker)
        }
        _ => Box::new(NoopReranker),
    }
}

#[cfg(feature = "embeddings")]
fn build_local(config: &RerankConfig) -> Option<Box<dyn Reranker>> {
    // Default model: BAAI/bge-reranker-base — solid English+Chinese baseline,
    // ~280MB. Override via config.model with strings like:
    //   "bge-reranker-base", "bge-reranker-v2-m3", "jina-v1-turbo-en",
    //   "jina-v2-multilingual"
    let model_name = config.model.clone().unwrap_or_else(|| "bge-reranker-base".into());
    Some(Box::new(LocalReranker { model_name }))
}

#[cfg(not(feature = "embeddings"))]
fn build_local(_config: &RerankConfig) -> Option<Box<dyn Reranker>> {
    None
}

fn build_anthropic(config: &RerankConfig) -> Option<Box<dyn Reranker>> {
    let key_env = config.api_key_env.as_deref().unwrap_or("ANTHROPIC_API_KEY");
    let key = std::env::var(key_env).ok().filter(|k| !k.is_empty())?;
    Some(Box::new(AnthropicReranker {
        api_key: key,
        model: config.model.clone().unwrap_or_else(|| "claude-haiku-4-5-20251001".into()),
    }))
}

fn build_openai(config: &RerankConfig) -> Option<Box<dyn Reranker>> {
    let key_env = config.api_key_env.as_deref().unwrap_or("OPENAI_API_KEY");
    let key = std::env::var(key_env).ok().filter(|k| !k.is_empty())?;
    Some(Box::new(OpenAIReranker {
        api_key: key,
        model: config.model.clone().unwrap_or_else(|| "gpt-4o-mini".into()),
    }))
}

fn build_claude_cli(config: &RerankConfig) -> Option<Box<dyn Reranker>> {
    let bin = which_on_path("claude")?;
    Some(Box::new(ClaudeCliReranker {
        binary: bin,
        model: config.model.clone().unwrap_or_else(|| "claude-haiku-4-5".into()),
    }))
}

fn build_codex_cli(config: &RerankConfig) -> Option<Box<dyn Reranker>> {
    let bin = which_on_path("codex")?;
    Some(Box::new(CodexCliReranker {
        binary: bin,
        model: config.model.clone().unwrap_or_else(|| "gpt-5-mini".into()),
    }))
}

/// Resolve a binary name on PATH. Avoids relying on the `which` crate.
///
/// On Windows, also probes `PATHEXT` (typically `.EXE;.CMD;.BAT;...`) so
/// callers can pass a bare name like `"claude"` and still find
/// `claude.exe`. Matches `cmd.exe`'s resolution rules.
fn which_on_path(name: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;

    #[cfg(windows)]
    let extensions: Vec<String> = {
        // If the caller already supplied an extension, just use it as-is;
        // otherwise iterate over PATHEXT plus a blank entry so `dir.join(name)`
        // also gets a shot (rare, but avoids a regression for bare names
        // that were resolvable before).
        let has_dot = std::path::Path::new(name).extension().is_some();
        if has_dot {
            vec![String::new()]
        } else {
            let pathext = std::env::var("PATHEXT")
                .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into());
            let mut exts: Vec<String> = pathext
                .split(';')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            exts.insert(0, String::new());
            exts
        }
    };
    #[cfg(not(windows))]
    let extensions: Vec<String> = vec![String::new()];

    for dir in std::env::split_paths(&path) {
        for ext in &extensions {
            let candidate = if ext.is_empty() {
                dir.join(name)
            } else {
                dir.join(format!("{}{}", name, ext))
            };
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Anthropic reranker. Sends top-K candidates to Haiku/Sonnet with the query
/// and asks for a ranked id list. Cost: ~$0.001/query for Haiku.
///
/// **Provider-aware prompts** are still TODO — for now we use one universal
/// prompt. Phase 3.5+ will add per-provider templates in
/// src-tauri/prompts/rerank/{haiku,sonnet,codex}.md.
pub struct AnthropicReranker {
    api_key: String,
    model: String,
}

impl Reranker for AnthropicReranker {
    fn rerank(&self, query: &str, hits: Vec<SearchHit>, top_k: usize) -> Vec<SearchHit> {
        if hits.is_empty() {
            return hits;
        }
        let limited: Vec<SearchHit> = hits.into_iter().take(20).collect();
        match call_anthropic(&self.api_key, &self.model, query, &limited) {
            Ok(order) => {
                let mut by_id: std::collections::HashMap<String, SearchHit> =
                    limited.into_iter().map(|h| (h.item.id.clone(), h)).collect();
                let mut out = Vec::new();
                for id in order {
                    if let Some(hit) = by_id.remove(&id) {
                        out.push(hit);
                    }
                    if out.len() >= top_k {
                        break;
                    }
                }
                // Append any unordered leftovers (in case the model dropped ids)
                out.extend(by_id.into_values());
                out.truncate(top_k);
                out
            }
            Err(e) => {
                log::warn!("[mem.rerank] anthropic call failed: {} — falling back to local order", e);
                let mut h = limited;
                h.truncate(top_k);
                h
            }
        }
    }
}

fn build_rerank_prompt(query: &str, hits: &[SearchHit]) -> String {
    let mut items = String::new();
    for h in hits {
        let summary = h.item.summary.as_deref().unwrap_or("");
        let snip: String = h.item.body.chars().take(200).collect();
        items.push_str(&format!(
            "- id={} kind={} title={}\n  summary={}\n  body={}\n\n",
            h.item.id, h.item.kind, h.item.title, summary, snip
        ));
    }
    crate::prompts::MEM_RERANK
        .replace("{query}", query)
        .replace("{items}", items.trim_end())
}

fn parse_id_array(text: &str) -> Result<Vec<String>, String> {
    let start = text.find('[').ok_or("no JSON array in response")?;
    let end = text.rfind(']').ok_or("unterminated JSON array")?;
    let json = &text[start..=end];
    serde_json::from_str(json).map_err(|e| format!("parse: {}", e))
}

fn call_anthropic(
    api_key: &str,
    model: &str,
    query: &str,
    hits: &[SearchHit],
) -> Result<Vec<String>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let prompt = build_rerank_prompt(query, hits);
    let body = serde_json::json!({
        "model": model,
        "max_tokens": 512,
        "messages": [{"role": "user", "content": prompt}],
    });
    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| e.to_string())?;
    let val: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
    let text = val.get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|m| m.get("text"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| "missing content[0].text".to_string())?;
    parse_id_array(text)
}

/// OpenAI / Codex CLI rerank. Uses /v1/chat/completions with a small model
/// (gpt-4o-mini default). For users running Codex CLI exclusively, the
/// same key works (Codex CLI uses OPENAI_API_KEY internally).
pub struct OpenAIReranker {
    api_key: String,
    model: String,
}

impl Reranker for OpenAIReranker {
    fn rerank(&self, query: &str, hits: Vec<SearchHit>, top_k: usize) -> Vec<SearchHit> {
        if hits.is_empty() {
            return hits;
        }
        let limited: Vec<SearchHit> = hits.into_iter().take(20).collect();
        match call_openai(&self.api_key, &self.model, query, &limited) {
            Ok(order) => {
                let mut by_id: std::collections::HashMap<String, SearchHit> =
                    limited.into_iter().map(|h| (h.item.id.clone(), h)).collect();
                let mut out = Vec::new();
                for id in order {
                    if let Some(hit) = by_id.remove(&id) {
                        out.push(hit);
                    }
                    if out.len() >= top_k {
                        break;
                    }
                }
                out.extend(by_id.into_values());
                out.truncate(top_k);
                out
            }
            Err(e) => {
                log::warn!("[mem.rerank] openai call failed: {} — falling back to local order", e);
                let mut h = limited;
                h.truncate(top_k);
                h
            }
        }
    }
}

/// Local cross-encoder reranker via fastembed-rs. Truly offline: model
/// downloads once (~280MB for BGE base, less for Jina turbo), then runs
/// inference in-process via ONNX. ~100-300ms per rerank for 20 candidates.
/// This is the default rerank backend — costs nothing per query.
#[cfg(feature = "embeddings")]
pub struct LocalReranker {
    model_name: String,
}

#[cfg(feature = "embeddings")]
impl Reranker for LocalReranker {
    fn rerank(&self, query: &str, hits: Vec<SearchHit>, top_k: usize) -> Vec<SearchHit> {
        if hits.is_empty() {
            return hits;
        }
        let limited: Vec<SearchHit> = hits.into_iter().take(20).collect();
        let docs: Vec<String> = limited.iter().map(|h| {
            // Use title + summary + body snippet — same shape that other
            // rerankers see in their text prompt.
            let summary = h.item.summary.as_deref().unwrap_or("");
            let snip: String = h.item.body.chars().take(400).collect();
            format!("{} | {} | {}", h.item.title, summary, snip)
        }).collect();

        match self.score_candidates(query, &docs) {
            Ok(scores) => {
                // Pair scores with hits, sort desc by score, truncate
                let mut paired: Vec<(f32, SearchHit)> =
                    scores.into_iter().zip(limited).collect();
                paired.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
                let mut out: Vec<SearchHit> = paired.into_iter().map(|(score, mut hit)| {
                    // Replace fused FTS score with reranker score (lower=better
                    // convention is preserved by negating, since reranker gives
                    // higher=better).
                    hit.score = -score as f64;
                    hit
                }).collect();
                out.truncate(top_k);
                out
            }
            Err(e) => {
                log::warn!("[mem.rerank] local fastembed failed: {} — falling back to local order", e);
                let mut h = limited;
                h.truncate(top_k);
                h
            }
        }
    }
}

#[cfg(feature = "embeddings")]
impl LocalReranker {
    fn score_candidates(&self, query: &str, docs: &[String]) -> Result<Vec<f32>, String> {
        use fastembed::{RerankInitOptions, RerankerModel, TextRerank};
        use parking_lot::Mutex;
        use std::sync::OnceLock;

        // Cache the loaded reranker model across calls (model load is the
        // expensive part). Indexed by model name.
        static MODEL_CACHE: OnceLock<Mutex<std::collections::HashMap<String, TextRerank>>> =
            OnceLock::new();
        let cache = MODEL_CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
        let mut guard = cache.lock();
        let model = match guard.get_mut(&self.model_name) {
            Some(m) => m,
            None => {
                let resolved = match self.model_name.as_str() {
                    "bge-reranker-base" | "BAAI/bge-reranker-base" => RerankerModel::BGERerankerBase,
                    "bge-reranker-v2-m3" | "rozgo/bge-reranker-v2-m3" => RerankerModel::BGERerankerV2M3,
                    "jina-v1-turbo-en" | "jinaai/jina-reranker-v1-turbo-en" => RerankerModel::JINARerankerV1TurboEn,
                    "jina-v2-multilingual" | "jinaai/jina-reranker-v2-base-multilingual" => RerankerModel::JINARerankerV2BaseMultiligual,
                    other => return Err(format!("unknown reranker model: {}", other)),
                };
                let opts = RerankInitOptions::new(resolved);
                let new_model = TextRerank::try_new(opts)
                    .map_err(|e| format!("load reranker: {}", e))?;
                guard.insert(self.model_name.clone(), new_model);
                guard.get_mut(&self.model_name).unwrap()
            }
        };

        // fastembed wants documents as Vec<String> via owned slice
        let owned: Vec<String> = docs.to_vec();
        let results = model
            .rerank(query.to_string(), owned, false, None)
            .map_err(|e| format!("rerank: {}", e))?;

        // results contains RerankResult { index, score, document } sorted by
        // score desc. We need scores in the original document order.
        let mut scores = vec![0f32; docs.len()];
        for r in results {
            if r.index < scores.len() {
                scores[r.index] = r.score;
            }
        }
        Ok(scores)
    }
}

/// Spawn a local `claude` CLI in print mode and ask it to rerank. Uses the
/// user's existing Claude Code authentication — no API key needed.
///
/// Command shape:
///   claude --bare --print --model <model> --output-format json "<prompt>"
/// `--bare` skips hooks, LSP, plugin sync, CLAUDE.md auto-discovery — fast.
/// `--output-format json` returns a structured envelope with the assistant
/// reply, which we parse for the inline JSON id array.
pub struct ClaudeCliReranker {
    binary: std::path::PathBuf,
    model: String,
}

impl Reranker for ClaudeCliReranker {
    fn rerank(&self, query: &str, hits: Vec<SearchHit>, top_k: usize) -> Vec<SearchHit> {
        if hits.is_empty() {
            return hits;
        }
        let limited: Vec<SearchHit> = hits.into_iter().take(20).collect();
        match call_claude_cli(&self.binary, &self.model, query, &limited) {
            Ok(order) => apply_order(limited, order, top_k),
            Err(e) => {
                log::warn!("[mem.rerank] claude-cli failed: {} — falling back to local order", e);
                let mut h = limited;
                h.truncate(top_k);
                h
            }
        }
    }
}

fn call_claude_cli(
    binary: &std::path::Path,
    model: &str,
    query: &str,
    hits: &[SearchHit],
) -> Result<Vec<String>, String> {
    let prompt = build_rerank_prompt(query, hits);
    let output = std::process::Command::new(binary)
        .arg("--bare")
        .arg("--print")
        .arg("--model").arg(model)
        .arg("--output-format").arg("json")
        .arg(&prompt)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("spawn claude: {}", e))?;
    if !output.status.success() {
        return Err(format!(
            "claude exit {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    // claude --output-format json wraps the assistant reply in a JSON envelope
    // like {"type": "result", "result": "..."} — we want `result`.
    let envelope: serde_json::Value =
        serde_json::from_str(&stdout).map_err(|e| format!("parse envelope: {}", e))?;
    let text = envelope.get("result")
        .and_then(|v| v.as_str())
        .or_else(|| envelope.get("response").and_then(|v| v.as_str()))
        .unwrap_or(stdout.as_ref());
    parse_id_array(text)
}

/// Spawn a local `codex` CLI in non-interactive `exec` mode for rerank.
/// Uses the user's existing codex auth — no API key needed.
pub struct CodexCliReranker {
    binary: std::path::PathBuf,
    model: String,
}

impl Reranker for CodexCliReranker {
    fn rerank(&self, query: &str, hits: Vec<SearchHit>, top_k: usize) -> Vec<SearchHit> {
        if hits.is_empty() {
            return hits;
        }
        let limited: Vec<SearchHit> = hits.into_iter().take(20).collect();
        match call_codex_cli(&self.binary, &self.model, query, &limited) {
            Ok(order) => apply_order(limited, order, top_k),
            Err(e) => {
                log::warn!("[mem.rerank] codex-cli failed: {} — falling back to local order", e);
                let mut h = limited;
                h.truncate(top_k);
                h
            }
        }
    }
}

fn call_codex_cli(
    binary: &std::path::Path,
    model: &str,
    query: &str,
    hits: &[SearchHit],
) -> Result<Vec<String>, String> {
    let prompt = build_rerank_prompt(query, hits);
    let output = std::process::Command::new(binary)
        .arg("exec")
        .arg("--model").arg(model)
        .arg(&prompt)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("spawn codex: {}", e))?;
    if !output.status.success() {
        return Err(format!(
            "codex exit {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_id_array(&stdout)
}

/// Apply an LLM-suggested id ordering to the candidate set, append any
/// missing ids at the end (model dropped them), truncate to top_k.
fn apply_order(limited: Vec<SearchHit>, order: Vec<String>, top_k: usize) -> Vec<SearchHit> {
    let mut by_id: std::collections::HashMap<String, SearchHit> =
        limited.into_iter().map(|h| (h.item.id.clone(), h)).collect();
    let mut out = Vec::new();
    for id in order {
        if let Some(hit) = by_id.remove(&id) {
            out.push(hit);
        }
        if out.len() >= top_k {
            break;
        }
    }
    out.extend(by_id.into_values());
    out.truncate(top_k);
    out
}

fn call_openai(
    api_key: &str,
    model: &str,
    query: &str,
    hits: &[SearchHit],
) -> Result<Vec<String>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let prompt = build_rerank_prompt(query, hits);
    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "temperature": 0.0,
        "max_tokens": 512,
        "response_format": {"type": "json_object"},
    });
    let resp = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("authorization", format!("Bearer {}", api_key))
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| e.to_string())?;
    let val: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
    // OpenAI's `response_format: json_object` requires the prompt to mention
    // "json" — ours does ("JSON array"). The model returns a JSON object;
    // we accept either {"ids": [...]} or a bare array if it slipped through.
    let text = val.get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|m| m.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| format!("missing choices[0].message.content in {}", val))?;
    // Try parsing as a JSON object first ({"ids": [...]} or {"order": [...]})
    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(text) {
        for key in &["ids", "order", "ranked", "result"] {
            if let Some(arr) = obj.get(*key).and_then(|v| v.as_array()) {
                let ids: Vec<String> = arr
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                if !ids.is_empty() { return Ok(ids); }
            }
        }
    }
    // Fallback: parse as bare array
    parse_id_array(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::store::MemItem;

    fn hit(id: &str) -> SearchHit {
        SearchHit {
            item: MemItem {
                id: id.into(), uid: "u".into(), kind: "fact".into(), scope: "global".into(),
                title: id.into(), summary: None, area: None, tags: vec![],
                body: id.into(), source: None, importance: 3.0,
                created_at: "".into(), updated_at: "".into(), accessed_at: None, access_count: 0, recurrence_count: 0,
                graph_x: None, graph_y: None,
            },
            score: 1.0,
        }
    }

    #[test]
    fn noop_preserves_order_truncated() {
        let r = NoopReranker;
        let hits = vec![hit("a"), hit("b"), hit("c")];
        let out = r.rerank("q", hits, 2);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].item.id, "a");
    }

    #[test]
    fn build_disabled_returns_noop() {
        let cfg = RerankConfig::default();
        let r = build(&cfg);
        let hits = vec![hit("a")];
        assert_eq!(r.rerank("q", hits, 5).len(), 1);
    }

    #[test]
    fn build_haiku_without_key_falls_back() {
        let cfg = RerankConfig {
            enabled: true,
            provider: "haiku".into(),
            api_key_env: Some("DEFINITELY_NOT_SET_KEY_12345".into()),
            model: None,
        };
        let r = build(&cfg);
        // Falls back to Noop because env var unset
        let hits = vec![hit("a"), hit("b")];
        let out = r.rerank("q", hits, 5);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn which_on_path_finds_known_binary() {
        // Every Rust test runs inside a process where `rustc` is reachable
        // via the same PATH we're about to query — use it as the "known
        // binary" so this test is deterministic on every CI host and every
        // developer machine regardless of how PATH is shaped. MSYS-style
        // forward-slash PATH, Windows backslash PATH, and plain Unix PATH
        // all land here.
        assert!(
            which_on_path("rustc").is_some(),
            "expected rustc to be resolvable on PATH (running test implies it is)"
        );
        assert!(which_on_path("definitely_not_a_real_binary_xyz").is_none());
    }

    #[test]
    fn build_claude_cli_falls_back_when_missing() {
        // Force PATH to a directory that has no claude
        let cfg = RerankConfig { enabled: true, provider: "claude-cli".into(), api_key_env: None, model: None };
        // Save and clear PATH so the test is deterministic
        let prev = std::env::var_os("PATH");
        std::env::set_var("PATH", "");
        let r = build(&cfg);
        if let Some(p) = prev { std::env::set_var("PATH", p); } else { std::env::remove_var("PATH"); }
        let hits = vec![hit("a")];
        assert_eq!(r.rerank("q", hits, 5).len(), 1);
    }

    #[test]
    fn auto_default_when_provider_empty() {
        let cfg = RerankConfig { enabled: true, provider: "".into(), api_key_env: Some("DEFINITELY_UNSET_KEY".into()), model: None };
        // Empty provider should fall through auto path. With no claude/codex
        // on PATH and no keys, lands on Noop. Just verify no panic.
        let prev = std::env::var_os("PATH");
        std::env::set_var("PATH", "");
        let r = build(&cfg);
        if let Some(p) = prev { std::env::set_var("PATH", p); } else { std::env::remove_var("PATH"); }
        let hits = vec![hit("a"), hit("b")];
        assert_eq!(r.rerank("q", hits, 5).len(), 2);
    }

    #[test]
    fn build_codex_without_key_falls_back() {
        let cfg = RerankConfig {
            enabled: true,
            provider: "codex".into(),
            api_key_env: Some("DEFINITELY_NOT_SET_KEY_67890".into()),
            model: None,
        };
        let r = build(&cfg);
        let hits = vec![hit("a")];
        assert_eq!(r.rerank("q", hits, 5).len(), 1);
    }

    #[test]
    fn build_auto_falls_back_when_no_keys() {
        let cfg = RerankConfig {
            enabled: true,
            provider: "auto".into(),
            // Use unset env vars so neither Anthropic nor OpenAI build
            api_key_env: Some("DEFINITELY_NOT_SET_AUTO_12345".into()),
            model: None,
        };
        let r = build(&cfg);
        let hits = vec![hit("a"), hit("b")];
        assert_eq!(r.rerank("q", hits, 5).len(), 2);
    }

    #[test]
    fn parse_id_array_works_on_pure_json() {
        let r = parse_id_array(r#"["a", "b", "c"]"#).unwrap();
        assert_eq!(r, vec!["a", "b", "c"]);
    }

    #[test]
    fn parse_id_array_extracts_from_prose() {
        let r = parse_id_array("Sure, here are the ranked ids: [\"x\", \"y\"] in best order.").unwrap();
        assert_eq!(r, vec!["x", "y"]);
    }
}
