use std::path::Path;

use candle_core::safetensors::MmapedSafetensors;
use candle_core::{DType, Device, Result, Shape, Tensor};
use candle_nn::var_builder::SimpleBackend;
use candle_nn::{embedding, layer_norm, Embedding, Init, LayerNorm, Module, VarBuilder};
use candle_transformers::models::bert::{BertEncoder, Config};

const VOCABULARY: &str = "embeddings.word_embeddings.weight";

pub(super) struct MappedBert {
    weights: MmapedSafetensors,
    parameters: Parameters,
}

struct Parameters {
    vocabulary: Vocabulary,
    position_embeddings: Embedding,
    token_type_embeddings: Embedding,
    layer_norm: LayerNorm,
    encoder: BertEncoder,
}

impl MappedBert {
    pub(super) fn load(path: &Path, config: &Config) -> Result<Self> {
        // Model files are immutable cache artifacts for the lifetime of the mapping.
        let weights = unsafe { MmapedSafetensors::new(path)? };
        let parameters = Parameters::load(&weights, config, None).or_else(|error| {
            let Some(prefix) = config.model_type.as_deref() else {
                return Err(error);
            };
            Parameters::load(&weights, config, Some(prefix)).map_err(|_| error)
        })?;
        Ok(Self {
            weights,
            parameters,
        })
    }

    pub(super) fn vocabulary_bytes(&self) -> usize {
        self.parameters.vocabulary.bytes
    }

    pub(super) fn forward(&self, ids: &[u32], types: &Tensor, mask: &Tensor) -> Result<Tensor> {
        let parameters = &self.parameters;
        let word = parameters.vocabulary.gather(&self.weights, ids)?;
        let token_type = parameters.token_type_embeddings.forward(types)?;
        let positions = Tensor::arange(0, ids.len() as u32, &Device::Cpu)?;
        let position = parameters.position_embeddings.forward(&positions)?;
        let hidden = (&word + &token_type)?.broadcast_add(&position)?;
        let hidden = parameters.layer_norm.forward(&hidden)?;
        let mask = mask.unsqueeze(1)?.unsqueeze(1)?.to_dtype(DType::F32)?;
        let mask = (mask.ones_like()? - &mask)?.broadcast_mul(&Tensor::try_from(f32::MIN)?)?;
        parameters.encoder.forward(&hidden, &mask)
    }
}

impl Parameters {
    fn load(weights: &MmapedSafetensors, config: &Config, prefix: Option<&str>) -> Result<Self> {
        let name = prefix.map_or_else(
            || VOCABULARY.into(),
            |prefix| format!("{prefix}.{VOCABULARY}"),
        );
        let vocabulary = Vocabulary::new(weights, name, config)?;
        let vars =
            VarBuilder::from_backend(Box::new(BorrowedWeights(weights)), DType::F32, Device::Cpu);
        let vars = match prefix {
            Some(prefix) => vars.pp(prefix),
            None => vars,
        };
        let embeddings = vars.pp("embeddings");
        Ok(Self {
            vocabulary,
            position_embeddings: embedding(
                config.max_position_embeddings,
                config.hidden_size,
                embeddings.pp("position_embeddings"),
            )?,
            token_type_embeddings: embedding(
                config.type_vocab_size,
                config.hidden_size,
                embeddings.pp("token_type_embeddings"),
            )?,
            layer_norm: layer_norm(
                config.hidden_size,
                config.layer_norm_eps,
                embeddings.pp("LayerNorm"),
            )?,
            encoder: BertEncoder::load(vars.pp("encoder"), config)?,
        })
    }
}

struct Vocabulary {
    name: String,
    dtype: DType,
    width: usize,
    row_bytes: usize,
    bytes: usize,
}

impl Vocabulary {
    fn new(weights: &MmapedSafetensors, name: String, config: &Config) -> Result<Self> {
        let view = weights.get(&name)?;
        if view.shape() != [config.vocab_size, config.hidden_size] || config.vocab_size == 0 {
            candle_core::bail!("invalid BERT vocabulary shape: {:?}", view.shape());
        }
        let bytes = view.data().len();
        let dtype = DType::try_from(view.dtype())?;
        let row_bytes = bytes / config.vocab_size;
        if config.hidden_size == 0 || row_bytes != config.hidden_size * dtype.size_in_bytes() {
            candle_core::bail!("invalid BERT vocabulary row size");
        }
        Ok(Self {
            name,
            dtype,
            width: config.hidden_size,
            row_bytes,
            bytes,
        })
    }

    fn gather(&self, weights: &MmapedSafetensors, ids: &[u32]) -> Result<Tensor> {
        let view = weights.get(&self.name)?;
        let rows = view.data().chunks_exact(self.row_bytes);
        let mut selected = Vec::with_capacity(ids.len() * self.row_bytes);
        for &id in ids {
            let row = rows.clone().nth(id as usize).ok_or_else(|| {
                candle_core::Error::Msg(format!("BERT token {id} outside vocabulary"))
            })?;
            selected.extend_from_slice(row);
        }
        Tensor::from_raw_buffer(
            &selected,
            self.dtype,
            &[1, ids.len(), self.width],
            &Device::Cpu,
        )?
        .to_dtype(DType::F32)
    }
}

struct BorrowedWeights<'a>(&'a MmapedSafetensors);

impl SimpleBackend for BorrowedWeights<'_> {
    fn get(
        &self,
        shape: Shape,
        name: &str,
        init: Init,
        dtype: DType,
        dev: &Device,
    ) -> Result<Tensor> {
        SimpleBackend::get(self.0, shape, name, init, dtype, dev)
    }

    fn get_unchecked(&self, name: &str, dtype: DType, dev: &Device) -> Result<Tensor> {
        SimpleBackend::get_unchecked(self.0, name, dtype, dev)
    }

    fn contains_tensor(&self, name: &str) -> bool {
        SimpleBackend::contains_tensor(self.0, name)
    }
}

#[cfg(test)]
mod tests;
