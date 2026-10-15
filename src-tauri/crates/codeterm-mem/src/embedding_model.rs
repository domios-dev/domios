use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config};
use tokenizers::Tokenizer;

pub(crate) const TOKENIZER_REPO: &str = crate::embeddings::EMBEDDER_MODEL_ID;

pub(crate) struct EmbeddingModel {
    model: BertModel,
    tokenizer: Tokenizer,
}

#[cfg(test)]
mod tests;

impl EmbeddingModel {
    pub(crate) fn load() -> Result<Self, String> {
        let files = crate::model_files::ModelFiles::new();
        let config: Config = serde_json::from_value(files.json(TOKENIZER_REPO, "config.json")?)
            .map_err(|e| e.to_string())?;
        if config.hidden_size != crate::embeddings::EMBEDDING_DIMENSION {
            return Err("unexpected embedding dimension".into());
        }
        let tokenizer = files.tokenizer(TOKENIZER_REPO, 512)?;
        let weights = files.get(crate::embeddings::EMBEDDER_MODEL_ID, "model.safetensors")?;
        let vb =
            unsafe { VarBuilder::from_mmaped_safetensors(&[weights], DType::F32, &Device::Cpu) }
                .map_err(|e| e.to_string())?;
        let model = BertModel::load(vb, &config).map_err(|e| e.to_string())?;
        files.remove_obsolete_onnx();
        Ok(Self { model, tokenizer })
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
        let mask = mask.to_dtype(DType::F32)?.unsqueeze(2)?;
        let pooled = hidden
            .broadcast_mul(&mask)?
            .sum(1)?
            .broadcast_div(&mask.sum(1)?)?;
        let vector = pooled.squeeze(0)?.to_vec1::<f32>()?;
        let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt() + 1e-12;
        Ok(vector.into_iter().map(|v| v / norm).collect())
    }
}
