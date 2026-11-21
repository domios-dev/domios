use super::*;
use crate::embedder::local::Pooling;
use crate::embedding_model::pool_embedding;
use candle_nn::VarMap;
use candle_transformers::models::bert::BertModel;

fn fixture(path: &Path, vocab_size: usize, prefix: Option<&str>) -> Config {
    let config = Config {
        vocab_size,
        hidden_size: 4,
        num_hidden_layers: 1,
        num_attention_heads: 2,
        intermediate_size: 8,
        max_position_embeddings: 8,
        ..Config::default()
    };
    let vars = VarMap::new();
    let builder = VarBuilder::from_varmap(&vars, DType::F32, &Device::Cpu);
    let builder = match prefix {
        Some(prefix) => builder.pp(prefix),
        None => builder,
    };
    BertModel::load(builder, &config).unwrap();
    vars.save(path).unwrap();
    config
}

#[test]
fn vocabulary_lookup_materializes_only_requested_rows() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("weights.safetensors");
    let config = fixture(&path, 16_384, None);
    let mapped = MappedBert::load(&path, &config).unwrap();
    let ids = [7, 2, 7, 16_383];
    let vocabulary = &mapped.parameters.vocabulary;
    let actual = vocabulary.gather(&mapped.weights, &ids).unwrap();
    assert_eq!(mapped.vocabulary_bytes(), 256 * 1024);
    assert_eq!(actual.elem_count() * actual.dtype().size_in_bytes(), 64);
    let expected = mapped
        .weights
        .load(VOCABULARY, &Device::Cpu)
        .unwrap()
        .index_select(&Tensor::new(&ids, &Device::Cpu).unwrap(), 0)
        .unwrap();
    assert_eq!(
        actual.flatten_all().unwrap().to_vec1::<f32>().unwrap(),
        expected.flatten_all().unwrap().to_vec1::<f32>().unwrap(),
    );
    assert!(vocabulary.gather(&mapped.weights, &[16_384]).is_err());
}

#[test]
fn mapped_bert_preserves_embedding_vectors() {
    let directory = tempfile::tempdir().unwrap();
    let ids = [7, 2, 7, 0];
    let input = Tensor::new(&[ids], &Device::Cpu).unwrap();
    let types = Tensor::new(&[[0u32, 1, 0, 1]], &Device::Cpu).unwrap();
    let mask_values = [1u32, 1, 1, 0];
    let mask = Tensor::new(&[mask_values], &Device::Cpu).unwrap();
    for (file, prefix) in [("root", None), ("bert", Some("bert"))] {
        let path = directory.path().join(format!("{file}.safetensors"));
        let config = fixture(&path, 8, prefix);
        let mapped = MappedBert::load(&path, &config).unwrap();
        let vars =
            unsafe { VarBuilder::from_mmaped_safetensors(&[path], DType::F32, &Device::Cpu) }
                .unwrap();
        let reference = BertModel::load(vars, &config).unwrap();
        let expected = reference.forward(&input, &types, Some(&mask)).unwrap();
        let actual = mapped.forward(&ids, &types, &mask).unwrap();
        assert_eq!(
            pool_embedding(&expected, &mask_values, Pooling::Mean).unwrap(),
            pool_embedding(&actual, &mask_values, Pooling::Mean).unwrap()
        );
    }
}
