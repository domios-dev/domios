use super::{DocCandidate, DocSection, Index};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Once, OnceLock};
use std::time::Instant;

const MAX_CHUNK_CHARS: usize = 4000;
const MAX_RANKED_CANDIDATES: usize = 20;
const RRF_K: usize = 60;
const MAX_HEADER_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct DocVectors {
    ids: Vec<String>,
    vectors: Vec<Vec<f32>>,
}

#[derive(Debug, Deserialize, Serialize)]
struct CacheHeader {
    chunk_count: usize,
    model: String,
    dim: usize,
    corpus_hash: String,
}

fn vector_store() -> &'static RwLock<Option<Arc<DocVectors>>> {
    static STORE: OnceLock<RwLock<Option<Arc<DocVectors>>>> = OnceLock::new();
    STORE.get_or_init(|| RwLock::new(None))
}

pub(super) fn ready_vectors() -> Option<Arc<DocVectors>> {
    vector_store().try_read()?.as_ref().cloned()
}

pub(super) fn start_background_build(data_dir: &Path) {
    static STARTED: Once = Once::new();
    STARTED.call_once(|| {
        let data_dir = data_dir.to_path_buf();
        if let Err(error) = std::thread::Builder::new()
            .name("docs-vector-build".into())
            .spawn(move || {
                let sections = super::corpus_sections();
                // A failure is already logged as unavailable; search keeps BM25.
                let _ = build_and_publish(&data_dir, sections);
            })
        {
            log_unavailable(&error.to_string());
        }
    });
}

pub(super) fn prepare_for_benchmark(data_dir: &Path, sections: &[DocSection]) -> bool {
    build_and_publish(data_dir, sections).is_ok()
}

fn build_and_publish(data_dir: &Path, sections: &[DocSection]) -> Result<(), String> {
    let started = Instant::now();
    let result = load_or_build(
        &data_dir.join("mem").join("docs-vectors"),
        sections,
        crate::embeddings::EMBEDDER_MODEL_ID,
        crate::embeddings::EMBEDDING_DIMENSION,
        &crate::embeddings::embed,
    );
    match result {
        Ok((vectors, cached)) => {
            let chunks = vectors.ids.len();
            publish_vectors(vectors);
            log::info!(
                "[docs.vectors] ready chunks={chunks} cached={cached} ms={}",
                started.elapsed().as_millis()
            );
            Ok(())
        }
        Err(error) => {
            log_unavailable(&error);
            Err(error)
        }
    }
}

fn log_unavailable(reason: &str) {
    log::warn!(
        "[docs.vectors] unavailable reason={}",
        reason.replace('\n', " ").replace('\r', " ")
    );
}

fn publish_vectors(vectors: DocVectors) {
    let mut store = vector_store().write();
    if store.is_none() {
        *store = Some(Arc::new(vectors));
    }
}

fn load_or_build<F>(
    cache_dir: &Path,
    sections: &[DocSection],
    model: &str,
    dim: usize,
    embedder: &F,
) -> Result<(DocVectors, bool), String>
where
    F: Fn(&str) -> Result<Vec<f32>, String>,
{
    let entries = corpus_entries(sections);
    let ids = entries.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>();
    let corpus_hash = corpus_hash(&entries, model);
    let path = cache_path(cache_dir, &corpus_hash);
    if let Ok(vectors) = read_cache(&path, &ids, &corpus_hash, model, dim) {
        return Ok((vectors, true));
    }

    let mut vectors = Vec::with_capacity(entries.len());
    for (id, text) in &entries {
        let vector = embedder(text).map_err(|error| format!("embed {id}: {error}"))?;
        validate_vector(&vector, dim).map_err(|error| format!("embed {id}: {error}"))?;
        vectors.push(vector);
    }
    let vectors = DocVectors { ids, vectors };
    write_cache(&path, &vectors, &corpus_hash, model, dim)?;
    Ok((vectors, false))
}

fn corpus_entries(sections: &[DocSection]) -> Vec<(String, String)> {
    sections
        .iter()
        .map(|section| {
            let id = section_id(section.doc_name, &section.anchor);
            let path = section.heading_path.join(" > ");
            let text = format!("{} > {}\n{}", section.doc_name, path, section.text)
                .chars()
                .take(MAX_CHUNK_CHARS)
                .collect();
            (id, text)
        })
        .collect()
}

fn section_id(doc_name: &str, anchor: &str) -> String {
    format!("{doc_name}\0{anchor}")
}

fn corpus_hash(entries: &[(String, String)], model: &str) -> String {
    let mut hasher = Sha256::new();
    hash_part(&mut hasher, model.as_bytes());
    for (id, text) in entries {
        hash_part(&mut hasher, id.as_bytes());
        hash_part(&mut hasher, text.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn hash_part(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn cache_path(cache_dir: &Path, corpus_hash: &str) -> PathBuf {
    cache_dir.join(format!("{corpus_hash}.bin"))
}

fn read_cache(
    path: &Path,
    expected_ids: &[String],
    corpus_hash: &str,
    model: &str,
    dim: usize,
) -> Result<DocVectors, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let mut reader = ByteReader::new(&bytes);
    let header_len = reader.read_u32()? as usize;
    if header_len > MAX_HEADER_BYTES {
        return Err("cache header too large".into());
    }
    let header: CacheHeader = serde_json::from_slice(reader.read_bytes(header_len)?)
        .map_err(|error| format!("parse cache header: {error}"))?;
    if header.corpus_hash != corpus_hash || header.model != model || header.dim != dim {
        return Err("cache header mismatch".into());
    }
    if header.chunk_count != expected_ids.len() {
        return Err("cache chunk count mismatch".into());
    }

    let mut ids = Vec::with_capacity(header.chunk_count);
    let mut vectors = Vec::with_capacity(header.chunk_count);
    for _ in 0..header.chunk_count {
        let id_len = reader.read_u32()? as usize;
        let id = std::str::from_utf8(reader.read_bytes(id_len)?)
            .map_err(|error| format!("parse cache id: {error}"))?
            .to_string();
        if id != expected_ids[ids.len()] {
            return Err("cache section id mismatch".into());
        }
        let vector = (0..dim)
            .map(|_| reader.read_f32())
            .collect::<Result<Vec<_>, _>>()?;
        validate_vector(&vector, dim)?;
        ids.push(id);
        vectors.push(vector);
    }
    if !reader.is_empty() {
        return Err("cache has trailing bytes".into());
    }
    Ok(DocVectors { ids, vectors })
}

fn write_cache(
    path: &Path,
    vectors: &DocVectors,
    corpus_hash: &str,
    model: &str,
    dim: usize,
) -> Result<(), String> {
    let header = CacheHeader {
        chunk_count: vectors.ids.len(),
        model: model.to_string(),
        dim,
        corpus_hash: corpus_hash.to_string(),
    };
    let header = serde_json::to_vec(&header).map_err(|error| error.to_string())?;
    let header_len = u32::try_from(header.len()).map_err(|_| "cache header too large")?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&header_len.to_le_bytes());
    bytes.extend_from_slice(&header);
    for (id, vector) in vectors.ids.iter().zip(&vectors.vectors) {
        let id_len = u32::try_from(id.len()).map_err(|_| "cache section id too large")?;
        bytes.extend_from_slice(&id_len.to_le_bytes());
        bytes.extend_from_slice(id.as_bytes());
        for value in vector {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }

    let parent = path
        .parent()
        .ok_or_else(|| "cache path has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|error| format!("create cache directory: {error}"))?;
    let temp_path = parent.join(format!(
        ".{}.{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("docs"),
        std::process::id(),
        ulid::Ulid::new()
    ));
    let mut temp = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .map_err(|error| format!("create cache temp file: {error}"))?;
    if let Err(error) = temp.write_all(&bytes).and_then(|_| temp.sync_all()) {
        let _ = fs::remove_file(&temp_path);
        return Err(format!("write cache temp file: {error}"));
    }
    drop(temp);
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            let _ = fs::remove_file(&temp_path);
            return Err(format!("remove stale cache: {error}"));
        }
    }
    if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(format!("publish cache: {error}"));
    }
    Ok(())
}

struct ByteReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> ByteReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn read_bytes(&mut self, count: usize) -> Result<&'a [u8], String> {
        let end = self
            .offset
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| "cache is truncated".to_string())?;
        let slice = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(slice)
    }

    fn read_u32(&mut self) -> Result<u32, String> {
        let bytes: [u8; 4] = self
            .read_bytes(4)?
            .try_into()
            .map_err(|_| "cache is truncated".to_string())?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn read_f32(&mut self) -> Result<f32, String> {
        let bytes: [u8; 4] = self
            .read_bytes(4)?
            .try_into()
            .map_err(|_| "cache is truncated".to_string())?;
        Ok(f32::from_le_bytes(bytes))
    }

    fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

fn validate_vector(vector: &[f32], dim: usize) -> Result<(), String> {
    if vector.len() != dim {
        return Err(format!("expected dimension {dim}, got {}", vector.len()));
    }
    if vector.iter().any(|value| !value.is_finite()) {
        return Err("vector contains a non-finite value".into());
    }
    if vector
        .iter()
        .map(|value| f64::from(*value).powi(2))
        .sum::<f64>()
        == 0.0
    {
        return Err("vector has zero magnitude".into());
    }
    Ok(())
}

pub(super) fn hybrid_search<F>(
    index: &Index,
    query: &str,
    limit: usize,
    vectors: &DocVectors,
    embedder: &F,
) -> Vec<DocCandidate>
where
    F: Fn(&str) -> Result<Vec<f32>, String>,
{
    hybrid_search_with_embedder(index, query, limit, Some(vectors), embedder)
}

fn hybrid_search_with_embedder<F>(
    index: &Index,
    query: &str,
    limit: usize,
    vectors: Option<&DocVectors>,
    embedder: &F,
) -> Vec<DocCandidate>
where
    F: Fn(&str) -> Result<Vec<f32>, String>,
{
    let fallback = || index.search(query, limit);
    if limit == 0 {
        return Vec::new();
    }
    let Some(vectors) = vectors else {
        return fallback();
    };
    if vectors.ids.len() != vectors.vectors.len() || vectors.ids.is_empty() {
        return fallback();
    }
    let dimension = vectors.vectors[0].len();
    let Ok(query_vector) = embedder(query) else {
        return fallback();
    };
    if validate_vector(&query_vector, dimension).is_err()
        || vectors
            .vectors
            .iter()
            .any(|vector| validate_vector(vector, dimension).is_err())
    {
        return fallback();
    }

    let bm25 = index.search(query, MAX_RANKED_CANDIDATES);
    let bm25_ranks = bm25
        .iter()
        .enumerate()
        .map(|(rank, candidate)| (section_id(candidate.doc_name, &candidate.anchor), rank + 1))
        .collect::<HashMap<_, _>>();
    let mut vector_ranks = vectors
        .ids
        .iter()
        .zip(&vectors.vectors)
        .enumerate()
        .map(|(index, (id, vector))| (id.clone(), index, cosine_similarity(&query_vector, vector)))
        .collect::<Vec<_>>();
    vector_ranks.sort_by(|(id_a, _, score_a), (id_b, _, score_b)| {
        score_b
            .total_cmp(score_a)
            .then_with(|| {
                bm25_ranks
                    .get(id_a)
                    .copied()
                    .unwrap_or(usize::MAX)
                    .cmp(&bm25_ranks.get(id_b).copied().unwrap_or(usize::MAX))
            })
            .then_with(|| id_a.cmp(id_b))
    });
    let vector_ranks = vector_ranks
        .into_iter()
        .take(MAX_RANKED_CANDIDATES)
        .enumerate()
        .map(|(rank, (id, _, _))| (id, rank + 1))
        .collect::<HashMap<_, _>>();

    let mut candidate_ids = HashSet::new();
    candidate_ids.extend(bm25_ranks.keys().cloned());
    candidate_ids.extend(vector_ranks.keys().cloned());
    let sections = index
        .sections
        .iter()
        .map(|indexed| {
            (
                section_id(indexed.section.doc_name, &indexed.section.anchor),
                &indexed.section,
            )
        })
        .collect::<HashMap<_, _>>();
    let mut ranked = candidate_ids
        .into_iter()
        .filter_map(|id| {
            let section = *sections.get(&id)?;
            let bm25_rank = bm25_ranks.get(&id).copied();
            let vector_rank = vector_ranks.get(&id).copied();
            let score = bm25_rank
                .map(|rank| 1.0 / (RRF_K + rank) as f64)
                .unwrap_or(0.0)
                + vector_rank
                    .map(|rank| 1.0 / (RRF_K + rank) as f64)
                    .unwrap_or(0.0);
            Some((score, bm25_rank, vector_rank, id, section))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(
        |(score_a, bm25_a, vector_a, id_a, _), (score_b, bm25_b, vector_b, id_b, _)| {
            score_b
                .total_cmp(score_a)
                .then_with(|| {
                    bm25_a
                        .unwrap_or(usize::MAX)
                        .cmp(&bm25_b.unwrap_or(usize::MAX))
                })
                .then_with(|| {
                    vector_a
                        .unwrap_or(usize::MAX)
                        .cmp(&vector_b.unwrap_or(usize::MAX))
                })
                .then_with(|| id_a.cmp(id_b))
        },
    );
    ranked
        .into_iter()
        .take(limit)
        .map(|(relevance, _, _, _, section)| DocCandidate {
            doc_name: section.doc_name,
            section_title: section.section_title.clone(),
            anchor: section.anchor.clone(),
            text: section.text.clone(),
            relevance,
        })
        .collect()
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> f64 {
    let dot = left
        .iter()
        .zip(right)
        .map(|(left, right)| f64::from(*left) * f64::from(*right))
        .sum::<f64>();
    let left_norm = left
        .iter()
        .map(|value| f64::from(*value).powi(2))
        .sum::<f64>()
        .sqrt();
    let right_norm = right
        .iter()
        .map(|value| f64::from(*value).powi(2))
        .sum::<f64>()
        .sqrt();
    dot / (left_norm * right_norm)
}

#[cfg(test)]
mod tests {
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
        let (built, cached) =
            load_or_build(dir.path(), &sections, "fake-model", 2, &embedder).unwrap();
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

            let (_, cached) =
                load_or_build(dir.path(), &sections, "fake-model", 2, &embedder).unwrap();
            assert!(!cached, "{field} mismatch must rebuild");
        }
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 3);
    }
}
