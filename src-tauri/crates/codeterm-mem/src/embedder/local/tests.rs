use super::{Content, Pooling, MODELS};

#[test]
fn embedder_candidates_prefixes_and_masked_pooling_match_fixture_vectors() {
    let prefixes = [
        ("", ""),
        ("query: ", "passage: "),
        (
            "Represent this sentence for searching relevant passages: ",
            "",
        ),
        ("query: ", "passage: "),
    ];
    let hidden = [[3.0, 4.0], [1.0, 2.0], [900.0, 900.0]];
    let content = Content::Text("Mixed input Пример".into());
    assert_eq!(MODELS.len(), 4);
    for (index, model) in MODELS.iter().enumerate() {
        assert_eq!((model.dimension, model.max_tokens), (384, 512));
        let (query, passage) = prefixes[index];
        assert_eq!(
            model.input(&content, true).unwrap(),
            format!("{query}Mixed input Пример")
        );
        assert_eq!(
            model.input(&content, false).unwrap(),
            format!("{passage}Mixed input Пример")
        );
        let weights = model.pooling.weights(&[1, 1, 0]).unwrap();
        let pooled: Vec<f32> = (0..2)
            .map(|column| {
                hidden
                    .iter()
                    .zip(&weights)
                    .map(|(row, weight)| row[column] * weight)
                    .sum()
            })
            .collect();
        let expected = if index == 2 {
            vec![3.0, 4.0]
        } else {
            vec![2.0, 3.0]
        };
        assert_eq!(pooled, expected, "{}", model.model);
    }
    assert!(Pooling::Mean.weights(&[]).is_err());
    assert!(Pooling::Mean.weights(&[0, 0]).is_err());
    assert_eq!(
        Pooling::Cls.weights(&[1, 1, 1]).unwrap(),
        vec![1.0, 0.0, 0.0]
    );
}
