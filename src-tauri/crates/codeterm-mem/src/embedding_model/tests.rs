#[test]
#[ignore = "downloads immutable model weights; explicitly run by rust-embeddings CI"]
fn candle_embedding_matches_fastembed_golden() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/fixtures/embed_parity.json")).unwrap();
    assert_eq!(fixture["model"], crate::embeddings::EMBEDDER_MODEL_ID);
    assert_eq!(fixture["dimension"], crate::embeddings::EMBEDDING_DIMENSION);
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 25);
    for (index, case) in cases.iter().enumerate() {
        let actual = crate::embeddings::embed(case["text"].as_str().unwrap()).unwrap();
        let expected: Vec<f64> = case["vector"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        assert_eq!(actual.len(), 384);
        assert_eq!(expected.len(), 384);
        let norm = actual
            .iter()
            .map(|&v| f64::from(v).powi(2))
            .sum::<f64>()
            .sqrt();
        assert!((norm - 1.0).abs() <= 1e-3, "case {index}: norm {norm}");
        let dot = actual
            .iter()
            .zip(&expected)
            .map(|(&a, &b)| f64::from(a) * b)
            .sum::<f64>();
        let expected_norm = expected.iter().map(|v| v * v).sum::<f64>().sqrt();
        let cosine = dot / (norm * expected_norm);
        println!("embedding parity case {index}: cosine={cosine:.9}");
        assert!(cosine >= 0.999, "case {index}: cosine {cosine}");
    }
}
