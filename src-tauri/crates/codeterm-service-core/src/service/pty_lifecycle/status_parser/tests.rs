use super::*;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Query {
    name: String,
    bytes: Vec<u8>,
    status_replies: usize,
}

#[test]
fn status_queries_match_xterm_corpus_at_every_byte_boundary() {
    let corpus: Vec<Query> = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tests/fixtures/terminal-status.json"
    )))
    .unwrap();
    assert!(!corpus.is_empty());
    for query in corpus {
        let expected = vec![STATUS_REPLY; query.status_replies];
        for split in 0..=query.bytes.len() {
            let mut parser = StatusParser::default();
            let mut replies = Vec::new();
            parser.feed(&query.bytes[..split], |reply| replies.push(reply));
            parser.feed(&query.bytes[split..], |reply| replies.push(reply));
            assert_eq!(replies, expected, "{} split={split}", query.name);
        }
        let mut parser = StatusParser::default();
        let mut replies = Vec::new();
        for byte in query.bytes.chunks(1) {
            parser.feed(byte, |reply| replies.push(reply));
        }
        assert_eq!(replies, expected, "{} bytewise", query.name);
    }
}
