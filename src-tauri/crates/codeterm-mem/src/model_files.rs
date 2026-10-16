use hf_hub::{api::sync::ApiBuilder, Cache};
use serde_json::Value;
use std::path::PathBuf;
use tokenizers::{AddedToken, PaddingParams, PaddingStrategy, Tokenizer, TruncationParams};

pub(crate) struct ModelFiles {
    cache: Cache,
}

impl ModelFiles {
    pub(crate) fn new() -> Self {
        let root = std::env::var_os("HF_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(crate::model_cache_dir);
        Self {
            cache: Cache::new(root),
        }
    }

    pub(crate) fn get(&self, repo: &str, file: &str) -> Result<PathBuf, String> {
        if let Some(path) = self.cache.model(repo.into()).get(file) {
            return Ok(path);
        }
        if std::env::var("HF_HUB_OFFLINE").is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        {
            return Err(format!("offline: {repo}/{file} is not cached"));
        }
        let endpoint =
            std::env::var("HF_ENDPOINT").unwrap_or_else(|_| "https://huggingface.co".into());
        ApiBuilder::from_cache(self.cache.clone())
            .with_endpoint(endpoint)
            .with_progress(false)
            .build()
            .map_err(|e| e.to_string())?
            .model(repo.into())
            .get(file)
            .map_err(|e| format!("download {repo}/{file}: {e}"))
    }

    pub(crate) fn json(&self, repo: &str, file: &str) -> Result<Value, String> {
        let bytes = std::fs::read(self.get(repo, file)?).map_err(|e| e.to_string())?;
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())
    }

    pub(crate) fn remove_obsolete_onnx(&self) {
        let obsolete = self
            .cache
            .path()
            .join("models--Xenova--paraphrase-multilingual-MiniLM-L12-v2");
        if obsolete.is_dir() {
            if let Err(error) = std::fs::remove_dir_all(&obsolete) {
                log::debug!(
                    "[embed] obsolete ONNX cache retained at {}: {error}",
                    obsolete.display()
                );
            }
        }
    }

    pub(crate) fn tokenizer(&self, repo: &str, max_length: usize) -> Result<Tokenizer, String> {
        let config = self.json(repo, "config.json")?;
        let special = self.json(repo, "special_tokens_map.json")?;
        let tokenizer_config = self.json(repo, "tokenizer_config.json")?;
        let mut tokenizer =
            Tokenizer::from_file(self.get(repo, "tokenizer.json")?).map_err(|e| e.to_string())?;
        let model_max = tokenizer_config["model_max_length"]
            .as_f64()
            .ok_or("missing tokenizer model_max_length")? as f32 as usize;
        tokenizer.with_padding(Some(PaddingParams {
            strategy: PaddingStrategy::BatchLongest,
            pad_id: config["pad_token_id"].as_u64().unwrap_or(0) as u32,
            pad_token: tokenizer_config["pad_token"]
                .as_str()
                .ok_or("missing pad_token")?
                .into(),
            ..Default::default()
        }));
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: max_length.min(model_max),
                ..Default::default()
            }))
            .map_err(|e| e.to_string())?;
        if let Some(special) = special.as_object() {
            for value in special.values() {
                let token = if let Some(content) = value.as_str() {
                    Some(AddedToken {
                        content: content.into(),
                        special: true,
                        ..Default::default()
                    })
                } else if let Some(object) = value.as_object() {
                    let mut object = object.clone();
                    object.insert("special".into(), Value::Bool(true));
                    serde_json::from_value::<AddedToken>(Value::Object(object)).ok()
                } else {
                    None
                };
                if let Some(token) = token {
                    tokenizer.add_special_tokens(&[token]);
                }
            }
        }
        Ok(tokenizer)
    }
}
