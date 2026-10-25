use super::*;
use tempfile::tempdir;

fn section(doc_name: &'static str, title: &str, anchor: &str, text: &str) -> DocSection {
    DocSection {
        doc_name,
        section_title: title.into(),
        anchor: anchor.into(),
        heading_path: vec![title.into()],
        text: text.into(),
    }
}

fn store(sections: &[DocSection], vectors: Vec<Vec<f32>>) -> DocVectors {
    DocVectors {
        ids: sections
            .iter()
            .map(|section| section_id(section.doc_name, &section.anchor))
            .collect(),
        vectors,
    }
}

#[test]
fn docs_vectors_absent_is_bm25_identical() {
    let index = Index::from_sections(vec![
        section("A", "Vault unlock", "vault-unlock", "unlock secret"),
        section("B", "Memory search", "memory-search", "search items"),
    ]);
    let expected = index.search("vault secret", 2);
    let actual = hybrid_search_with_embedder(&index, "vault secret", 2, None, &|_| {
        panic!("absent vectors must not embed")
    });
    assert_eq!(actual, expected);
}

#[test]
fn docs_vectors_rrf_promotes_vector_only_section() {
    let mut sections = vec![
        section("A", "Alpha first", "alpha-first", "alpha alpha"),
        section("B", "Alpha second", "alpha-second", "alpha"),
        section("C", "Vector match", "vector-match", "unrelated text"),
    ];
    sections.extend((0..20).map(|index| {
        section(
            "D",
            &format!("Vector distractor {index}"),
            &format!("vector-distractor-{index}"),
            "unrelated text",
        )
    }));
    let index = Index::from_sections(sections.clone());
    let bm25 = index.search("alpha", 2);
    assert_eq!(bm25.len(), 2);
    let vectors = store(
        &sections,
        sections
            .iter()
            .map(|section| {
                if section.anchor == "vector-match" {
                    vec![1.0, 0.0]
                } else if section.anchor.starts_with("vector-distractor") {
                    vec![0.9, 0.1]
                } else {
                    vec![-1.0, 0.0]
                }
            })
            .collect(),
    );

    let query_calls = std::sync::atomic::AtomicUsize::new(0);
    let result = hybrid_search(&index, "alpha", 2, &vectors, &|_| {
        query_calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(vec![1.0, 0.0])
    });

    assert_eq!(query_calls.load(std::sync::atomic::Ordering::Relaxed), 1);
    assert_eq!(result.len(), 2);
    assert!(result
        .iter()
        .any(|candidate| candidate.anchor == "vector-match"));
    assert_eq!(
        vectors
            .ids
            .iter()
            .zip(&vectors.vectors)
            .max_by(|(_, a), (_, b)| cosine_similarity(&[1.0, 0.0], a)
                .total_cmp(&cosine_similarity(&[1.0, 0.0], b)))
            .map(|(id, _)| id.as_str()),
        Some("C\0vector-match")
    );
    assert!(!bm25
        .iter()
        .any(|candidate| candidate.anchor == "vector-match"));
}

#[test]
fn docs_vectors_ties_break_by_bm25_rank() {
    let sections = vec![
        section("A", "First", "first", "alpha"),
        section("B", "Second", "second", "alpha"),
    ];
    let index = Index::from_sections(sections.clone());
    let vectors = store(&sections, vec![vec![-1.0], vec![1.0]]);

    let result = hybrid_search(&index, "alpha", 2, &vectors, &|_| Ok(vec![1.0]));

    assert_eq!(result[0].anchor, "first");
    assert_eq!(result[1].anchor, "second");
}

#[test]
fn docs_vectors_query_embed_error_falls_back() {
    let sections = vec![
        section("A", "Vault unlock", "vault-unlock", "unlock secret"),
        section("B", "Memory search", "memory-search", "search items"),
    ];
    let index = Index::from_sections(sections.clone());
    let vectors = store(&sections, vec![vec![1.0], vec![2.0]]);
    let expected = index.search("vault secret", 2);

    let actual = hybrid_search(&index, "vault secret", 2, &vectors, &|_| {
        Err("fake query failure".into())
    });

    assert_eq!(actual, expected);
}

#[test]
fn docs_vectors_cache_round_trip_keyed_by_hash() {
    let dir = tempdir().unwrap();
    let sections = vec![section("A", "Vault", "vault", "unlock secret")];
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let embedder = |text: &str| {
        calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(vec![text.len() as f32, 1.0])
    };
    let (built, cached) = load_or_build(dir.path(), &sections, "fake-model", 2, &embedder).unwrap();
    assert!(!cached);
    assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
    let entries = corpus_entries(&sections);
    let hash = corpus_hash(&entries, "fake-model");
    let expected_file = format!("{hash}.bin");
    assert_eq!(
        cache_path(dir.path(), &hash)
            .file_name()
            .and_then(|name| name.to_str()),
        Some(expected_file.as_str())
    );

    let (loaded, cached) = load_or_build(dir.path(), &sections, "fake-model", 2, &|_| {
        panic!("cache hit must not embed")
    })
    .unwrap();

    assert!(cached);
    assert_eq!(loaded, built);
    assert_ne!(
        cache_path(dir.path(), &corpus_hash(&entries, "other-model")),
        cache_path(dir.path(), &hash)
    );
    let changed = vec![section("A", "Vault", "vault", "changed text")];
    assert_ne!(
        cache_path(
            dir.path(),
            &corpus_hash(&corpus_entries(&changed), "fake-model")
        ),
        cache_path(dir.path(), &hash)
    );
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn docs_vectors_cache_mismatch_rebuilds() {
    let dir = tempdir().unwrap();
    let sections = vec![section("A", "Vault", "vault", "unlock secret")];
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let embedder = |_: &str| {
        calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(vec![1.0, 2.0])
    };
    let (_, cached) = load_or_build(dir.path(), &sections, "fake-model", 2, &embedder).unwrap();
    assert!(!cached);
    assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
    let entries = corpus_entries(&sections);
    let hash = corpus_hash(&entries, "fake-model");
    let path = cache_path(dir.path(), &hash);

    for (field, value) in [("corpus_hash", "wrong-hash"), ("dim", "3")] {
        let bytes = fs::read(&path).unwrap();
        let header_len = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
        let mut header: serde_json::Value =
            serde_json::from_slice(&bytes[4..4 + header_len]).unwrap();
        header[field] = if field == "dim" {
            serde_json::json!(value.parse::<usize>().unwrap())
        } else {
            serde_json::json!(value)
        };
        let header = serde_json::to_vec(&header).unwrap();
        let mut mutated = (header.len() as u32).to_le_bytes().to_vec();
        mutated.extend_from_slice(&header);
        mutated.extend_from_slice(&bytes[4 + header_len..]);
        fs::write(&path, mutated).unwrap();

        let (_, cached) = load_or_build(dir.path(), &sections, "fake-model", 2, &embedder).unwrap();
        assert!(!cached, "{field} mismatch must rebuild");
    }
    assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 3);
}

#[test]
fn recall_reembed_docs_build_pins_provider_across_switch() {
    use crate::embedder::testing::{MockProvider, DEADLINE};
    use crate::embedder::{ActiveEmbedder, EmbeddingProvider};
    let dir = tempdir().unwrap();
    let old = MockProvider::new("old", "old", 8);
    let active = ActiveEmbedder::new(old.clone());
    let (started, release) = old.gated();
    let building = active.clone();
    let sections = crate::docs_source::fixture_sections();
    let count = sections.len();
    let thread =
        std::thread::spawn(move || prepare(dir.path(), &building, &sections, || {}).unwrap());
    started.recv_timeout(DEADLINE).unwrap();
    let new = MockProvider::new("new", "new", 16);
    active.replace(new.clone());
    old.ungate();
    release.send(()).unwrap();
    let prepared = thread.join().unwrap();
    assert_eq!(prepared.vectors.vectors.len(), count);
    assert_eq!(old.calls.load(std::sync::atomic::Ordering::SeqCst), count);
    assert_eq!(new.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(prepared.embedder.identity(), old.identity());
    assert!(prepared
        .vectors
        .vectors
        .iter()
        .all(|vector| vector.len() == 8));
    assert_eq!(active.identity().dimension, 16);
}

#[test]
fn docs_vectors_incremental_cache_reuses_unchanged_sections_after_edits_and_failure() {
    let dir = tempdir().unwrap();
    let mut sections = vec![
        section("A", "First", "first", "original"),
        section("B", "Second", "second", "original"),
    ];
    let calls = std::cell::RefCell::new(Vec::new());
    let embed = |text: &str| {
        calls.borrow_mut().push(text.to_owned());
        Ok(vec![1.0, 2.0])
    };
    load_or_build(dir.path(), &sections, "model", 2, &embed).unwrap();
    assert_eq!(calls.borrow().len(), 2);
    calls.borrow_mut().clear();
    sections[1].text = "changed".into();
    load_or_build(dir.path(), &sections, "model", 2, &embed).unwrap();
    assert_eq!(*calls.borrow(), vec!["B > Second\nchanged"]);
    sections.push(section("C", "Third", "third", "new"));
    assert!(load_or_build(
        dir.path(),
        &sections,
        "model",
        2,
        &|_| Err("offline".into())
    )
    .is_err());
    calls.borrow_mut().clear();
    load_or_build(dir.path(), &sections, "model", 2, &embed).unwrap();
    assert_eq!(*calls.borrow(), vec!["C > Third\nnew"]);
    load_or_build(dir.path(), &sections, "model", 2, &|_| {
        panic!("unchanged boot embedded")
    })
    .unwrap();
    calls.borrow_mut().clear();
    load_or_build(dir.path(), &sections, "different", 2, &embed).unwrap();
    assert_eq!(calls.borrow().len(), 3);
}

#[test]
fn docs_vectors_split_unicode_into_bounded_inferences_and_pool_all_chunks() {
    let calls = std::cell::Cell::new(0);
    let vector = embed_section(&"界".repeat(INFERENCE_CHARS * 2 + 1), 2, &|text| {
        assert!(text.chars().count() <= INFERENCE_CHARS);
        calls.set(calls.get() + 1);
        Ok(vec![1.0, 0.0])
    })
    .unwrap();
    assert_eq!(calls.get(), 3);
    assert_eq!(vector, vec![1.0, 0.0]);
}

#[test]
fn docs_vectors_completed_sections_survive_an_interrupted_initial_build() {
    let dir = tempdir().unwrap();
    let sections = vec![
        section("A", "First", "first", "first content"),
        section("B", "Second", "second", "second content"),
    ];
    assert!(load_or_build(dir.path(), &sections, "model", 2, &|text| {
        if text.starts_with("B >") {
            Err("interrupted".into())
        } else {
            Ok(vec![1.0, 0.0])
        }
    })
    .is_err());
    let progress = std::cell::Cell::new(0);
    let calls = std::cell::RefCell::new(Vec::new());
    load_or_build_with_progress(
        dir.path(),
        &sections,
        "model",
        2,
        &|text| {
            calls.borrow_mut().push(text.to_owned());
            Ok(vec![1.0, 0.0])
        },
        || progress.set(progress.get() + 1),
    )
    .unwrap();
    assert_eq!(*calls.borrow(), vec!["B > Second\nsecond content"]);
    assert_eq!(progress.get(), 2);
}
