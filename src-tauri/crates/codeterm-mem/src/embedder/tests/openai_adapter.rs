use std::sync::Arc;

use super::mock_http::MockServer;
use crate::embedder::key::{ApiKey, CachedKey, KeySources};
use crate::embedder::openai::{self, OpenAiProvider};
use crate::embedder::provider::{Content, EmbedError, EmbeddingProvider};
use crate::embedder::testing::{fixture_for, note, DEADLINE};
use crate::embedder::ActiveEmbedder;

fn provider(server: &MockServer, dimension: usize) -> Arc<OpenAiProvider> {
    let key = CachedKey::seeded(
        KeySources::new(std::path::PathBuf::new(), None),
        openai::ID,
        ApiKey::new("test-key-123").unwrap(),
    );
    Arc::new(
        OpenAiProvider::new(
            openai::find("text-embedding-3-small").unwrap(),
            dimension,
            &server.url,
            key,
        )
        .unwrap(),
    )
}

#[test]
fn local_mock_server_smoke_covers_shape_batching_errors_and_the_credential_filter() {
    let server = MockServer::embeddings(256);
    let adapter = provider(&server, 256);
    let contents: Vec<Content> = (0..130)
        .map(|i| Content::text(format!("chunk {i}")))
        .collect();
    let vectors = adapter.embed_passages(&contents);
    assert_eq!(server.count(), 3, "64 + 64 + 2 inputs");
    for (index, vector) in vectors.iter().enumerate() {
        assert_eq!(
            vector.as_ref().unwrap()[0],
            (index % openai::BATCH_SIZE) as f32 + 1.0
        );
    }
    let request = server.last();
    assert_eq!(request.path, "/v1/embeddings");
    assert_eq!(
        request.authorization.as_deref(),
        Some("Bearer test-key-123")
    );
    assert_eq!(request.body["model"], "text-embedding-3-small");
    assert_eq!(request.body["dimensions"], 256);
    assert_eq!(request.body["input"].as_array().unwrap().len(), 2);

    let long = "word ".repeat(20_000);
    adapter.embed_passage(&Content::text(long)).unwrap();
    let sent = server.last().body["input"][0].as_str().unwrap().to_string();
    assert!(openai::estimate_tokens(&sent) <= 8191);

    let active = ActiveEmbedder::new(adapter.clone());
    let before = server.count();
    let blocked = active.passage("deploy with api_key = abcdefghijklmnop");
    assert!(matches!(blocked, Err(EmbedError::Refused(_))));
    assert_eq!(server.count(), before, "no call for filtered text");

    for status in [401u16, 429, 503] {
        let failing = MockServer::status(status);
        let fx = fixture_for(provider(&failing, 64));
        fx.mem
            .save(note("Remote note", "text for the remote embedder"))
            .unwrap();
        assert!(fx.mem.embed_queue.wait_idle(DEADLINE));
        assert_eq!(
            fx.mem.embedder_status().pending,
            1,
            "status {status} is vector pending"
        );
    }
}
