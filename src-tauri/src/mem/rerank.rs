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
            provider: "noop".into(),
            api_key_env: None,
            model: None,
        }
    }
}

/// Build a Reranker from config. Falls back to Noop on misconfiguration.
/// Provider precedence:
/// - "haiku" | "sonnet" | "anthropic" → AnthropicReranker (needs ANTHROPIC_API_KEY)
/// - "codex" | "openai" → OpenAIReranker (needs OPENAI_API_KEY, model defaults to gpt-4o-mini)
/// - "auto" → tries Anthropic first, then OpenAI, then Noop
/// - anything else → Noop
pub fn build(config: &RerankConfig) -> Box<dyn Reranker> {
    if !config.enabled {
        return Box::new(NoopReranker);
    }
    match config.provider.as_str() {
        "haiku" | "sonnet" | "anthropic" => build_anthropic(config).unwrap_or_else(|| {
            log::warn!("[mem.rerank] anthropic unavailable, falling back to Noop");
            Box::new(NoopReranker)
        }),
        "codex" | "openai" | "gpt" => build_openai(config).unwrap_or_else(|| {
            log::warn!("[mem.rerank] openai unavailable, falling back to Noop");
            Box::new(NoopReranker)
        }),
        "auto" => {
            if let Some(r) = build_anthropic(config) { return r; }
            if let Some(r) = build_openai(config) { return r; }
            log::warn!("[mem.rerank] auto: no API key found, falling back to Noop");
            Box::new(NoopReranker)
        }
        _ => Box::new(NoopReranker),
    }
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

/// Provider-aware rerank prompt builder. Both Anthropic and OpenAI variants
/// use the same input format — the prompt itself can be tuned per-provider
/// later if we hit specific failure modes. For now: short, instruction-only,
/// no chain-of-thought (Haiku and gpt-4o-mini are both fast at this task).
fn build_rerank_prompt(query: &str, hits: &[SearchHit]) -> String {
    let mut prompt = format!(
        "You are reranking memory items for an agent's query. Return ONLY a JSON array of item ids in best-to-worst relevance order. No prose, no markdown, just the array.\n\nQuery: {}\n\nItems:\n",
        query
    );
    for h in hits {
        let summary = h.item.summary.as_deref().unwrap_or("");
        let snip: String = h.item.body.chars().take(200).collect();
        prompt.push_str(&format!(
            "- id={} kind={} title={}\n  summary={}\n  body={}\n\n",
            h.item.id, h.item.kind, h.item.title, summary, snip
        ));
    }
    prompt.push_str("\nReturn the JSON array only, e.g. [\"id1\", \"id2\", \"id3\"].");
    prompt
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
                created_at: "".into(), updated_at: "".into(), accessed_at: None,
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
