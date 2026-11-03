use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config};
use tokenizers::Tokenizer;

use crate::embedder::local::{LocalModel, Pooling};
use std::sync::atomic::{AtomicUsize, Ordering};

static RESIDENT_MODELS: AtomicUsize = AtomicUsize::new(0);
static MAPPED_WEIGHT_BYTES: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn memory_diagnostics() -> serde_json::Value {
    serde_json::json!({
        "count": RESIDENT_MODELS.load(Ordering::Relaxed),
        "mapped_weight_bytes": MAPPED_WEIGHT_BYTES.load(Ordering::Relaxed),
        "residency": "lazy loaded; at most two models retained by the embedding worker",
    })
}

pub(crate) struct EmbeddingModel {
    model: BertModel,
    tokenizer: Tokenizer,
    pooling: Pooling,
    weight_bytes: usize,
}

#[cfg(test)]
mod tests;

impl EmbeddingModel {
    pub(crate) fn load(descriptor: &'static LocalModel) -> Result<Self, String> {
        let files = crate::model_files::ModelFiles::new(descriptor.revision);
        let repo = descriptor.model;
        let config: Config =
            serde_json::from_value(files.json(repo, "config.json")?).map_err(|e| e.to_string())?;
        if config.hidden_size != descriptor.dimension {
            return Err("unexpected embedding dimension".into());
        }
        let tokenizer = files.tokenizer(repo, descriptor.max_tokens)?;
        let weights = files.get(repo, "model.safetensors")?;
        let weight_bytes = std::fs::metadata(&weights)
            .map_err(|error| error.to_string())?
            .len() as usize;
        let vb =
            unsafe { VarBuilder::from_mmaped_safetensors(&[weights], DType::F32, &Device::Cpu) }
                .map_err(|e| e.to_string())?;
        let model = BertModel::load(vb, &config).map_err(|e| e.to_string())?;
        files.remove_obsolete_onnx();
        RESIDENT_MODELS.fetch_add(1, Ordering::Relaxed);
        MAPPED_WEIGHT_BYTES.fetch_add(weight_bytes, Ordering::Relaxed);
        Ok(Self {
            model,
            tokenizer,
            pooling: descriptor.pooling,
            weight_bytes,
        })
    }

    pub(crate) fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
        self.forward(text).map_err(|e| e.to_string())
    }

    fn forward(&self, text: &str) -> candle_core::Result<Vec<f32>> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| candle_core::Error::Msg(e.to_string()))?;
        let ids = Tensor::new(encoding.get_ids(), &Device::Cpu)?.unsqueeze(0)?;
        let types = Tensor::new(encoding.get_type_ids(), &Device::Cpu)?.unsqueeze(0)?;
        let mask = Tensor::new(encoding.get_attention_mask(), &Device::Cpu)?.unsqueeze(0)?;
        let hidden = self.model.forward(&ids, &types, Some(&mask))?;
        let weights = self
            .pooling
            .weights(encoding.get_attention_mask())
            .map_err(candle_core::Error::Msg)?;
        let weights = Tensor::new(weights, &Device::Cpu)?.reshape((1, encoding.len(), 1))?;
        let pooled = hidden.broadcast_mul(&weights)?.sum(1)?;
        let vector = pooled.squeeze(0)?.to_vec1::<f32>()?;
        let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt() + 1e-12;
        Ok(vector.into_iter().map(|v| v / norm).collect())
    }
}

impl Drop for EmbeddingModel {
    fn drop(&mut self) {
        RESIDENT_MODELS.fetch_sub(1, Ordering::Relaxed);
        MAPPED_WEIGHT_BYTES.fetch_sub(self.weight_bytes, Ordering::Relaxed);
    }
}
