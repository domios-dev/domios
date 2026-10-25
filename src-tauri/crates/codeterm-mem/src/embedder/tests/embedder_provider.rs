use std::sync::Arc;

use crate::embedder::catalog::{self, BuildContext, BuildError, EmbedderSelection};
use crate::embedder::key::KeySources;
use crate::embedder::local::{self, LocalProvider, Pooling};
use crate::embedder::provider::{Content, EmbedError, EmbeddingProvider, Modality};
use crate::embedder::testing::MockProvider;
use crate::embedder::{openai, ActiveEmbedder};

fn ctx() -> (tempfile::TempDir, BuildContext) {
    let dir = tempfile::tempdir().unwrap();
    let keys = KeySources::new(dir.path().join("secrets"), None);
    (
        dir,
        BuildContext {
            keys,
            endpoint: None,
        },
    )
}

#[test]
fn registry_lists_the_local_default_and_the_remote_adapter_by_capability() {
    let catalog = catalog::catalog();
    let local_row = catalog.iter().find(|row| row.id == local::ID).unwrap();
    assert!(!local_row.remote);
    assert_eq!(local_row.modalities, vec![Modality::Text]);
    let default = local_row.model(None).unwrap();
    assert_eq!(
        (default.model.as_str(), default.dimension),
        (
            "sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2",
            384
        )
    );
    let remote_row = catalog.iter().find(|row| row.id == openai::ID).unwrap();
    assert!(remote_row.remote);
    let models: Vec<_> = remote_row.models.iter().map(|m| m.model.as_str()).collect();
    assert_eq!(models, ["text-embedding-3-small", "text-embedding-3-large"]);
    assert_eq!(remote_row.model(None).unwrap().dimension, 1536);
}

#[test]
fn default_selection_builds_the_table_driven_l12_descriptor() {
    let (_dir, ctx) = ctx();
    let built = catalog::build(&EmbedderSelection::default(), &ctx).unwrap();
    assert_eq!(built.identity(), crate::embedder::legacy_identity());
    assert_eq!(built.dimension(), 384);
    assert_eq!(built.max_tokens(), 512);
    assert!(!built.remote());
    let row = local::default_model();
    assert_eq!(row.revision, "main");
    assert_eq!(row.pooling, Pooling::Mean);
    assert_eq!((row.query_prefix, row.passage_prefix), ("", ""));
    assert_eq!(ActiveEmbedder::global().identity(), built.identity());
}

#[test]
fn selections_outside_the_registry_are_refused() {
    let (_dir, ctx) = ctx();
    let unknown = EmbedderSelection {
        provider: "nope".into(),
        ..EmbedderSelection::default()
    };
    assert_eq!(
        catalog::build(&unknown, &ctx).err(),
        Some(BuildError::UnknownProvider("nope".into()))
    );
    let bad_model = EmbedderSelection {
        model: Some("missing-model".into()),
        ..EmbedderSelection::default()
    };
    assert!(matches!(
        catalog::build(&bad_model, &ctx),
        Err(BuildError::UnknownModel { .. })
    ));
    let bad_dimension = EmbedderSelection {
        provider: openai::ID.into(),
        model: Some("text-embedding-3-small".into()),
        dimension: Some(4096),
    };
    assert!(matches!(
        catalog::build(&bad_dimension, &ctx),
        Err(BuildError::InvalidDimension(_))
    ));
}

#[test]
fn text_only_providers_refuse_image_content_through_their_modalities() {
    let image = Content::Image {
        mime: "image/png".into(),
        bytes: vec![1, 2, 3],
    };
    let local_provider = LocalProvider::new(local::default_model());
    assert!(!local_provider.modalities().contains(&Modality::Image));
    assert!(matches!(
        local_provider.embed_passage(&image),
        Err(EmbedError::Refused(_))
    ));
    let mock = MockProvider::new("mock", "m", 8);
    assert!(matches!(
        mock.embed_query(&image),
        Err(EmbedError::Refused(_))
    ));
    assert_eq!(mock.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn every_vector_carries_the_identity_of_the_provider_that_made_it() {
    let first = MockProvider::new("mock-a", "model-a", 8);
    let active = ActiveEmbedder::new(first);
    let stamped = active.passage("Alpha Beta").unwrap();
    assert_eq!(stamped.identity.provider, "mock-a");
    assert_eq!(stamped.vector.len(), 8);
    active.replace(MockProvider::new("mock-b", "model-b", 16));
    let query = active.query("alpha").unwrap();
    assert_eq!(
        (query.identity.model.as_str(), query.vector.len()),
        ("model-b", 16)
    );
}

#[test]
fn only_one_provider_is_active_and_replacing_it_announces_the_switch() {
    let active = ActiveEmbedder::new(MockProvider::new("mock-a", "a", 4));
    let (sender, events) = std::sync::mpsc::channel();
    let sender = parking_lot::Mutex::new(sender);
    active.subscribe(Arc::new(move |event| {
        let _ = sender.lock().send(event.clone());
    }));
    let next = MockProvider::new("mock-b", "b", 4);
    active.replace(next.clone());
    let event = events.try_recv().unwrap();
    assert_eq!(
        event,
        crate::embedder::EmbedderEvent::Switched(next.identity())
    );
    assert_eq!(active.identity(), next.identity());
}
