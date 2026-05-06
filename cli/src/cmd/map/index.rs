use crate::cmd::map::bridge::http::{extract_calls, extract_handlers, match_path};
use crate::cmd::map::bridge::tauri::{handlers_in_rust, invokes_in_ts};
use crate::cmd::map::cache::{check_file, fs_mtime_ns, fs_size};
use crate::cmd::map::chunks::chunk_file;
use crate::cmd::map::embed::{vec_to_le_bytes, EmbedClient, VECTOR_DIM};
use crate::cmd::map::lang::Lang;
use crate::cmd::map::pagerank::{personalized as pagerank, EdgeWeight};
use crate::cmd::map::store::{NewNode, Store};
use crate::cmd::map::tags::{extract, Tag, TagRole};
use crate::cmd::map::walker::{walk_workspace, WorkspaceFile};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Default, serde::Serialize)]
pub struct IndexReport {
    pub files: usize,
    pub files_indexed: usize,
    pub files_cached: usize,
    pub nodes: usize,
    pub edges: usize,
    pub errors: usize,
    pub chunks: usize,
    pub chunks_embedded: usize,
    pub elapsed_ms: u128,
}

pub struct IndexOptions {
    pub force: bool,
    pub verbose: bool,
    pub progress: bool,
}

pub fn run_index(root: &Path, opts: IndexOptions) -> Result<IndexReport, String> {
    let started = Instant::now();
    let store = Store::open(root).map_err(|e| format!("open store: {e}"))?;
    store
        .set_meta(
            "last_index_started_at",
            &chrono::Utc::now().timestamp_millis().to_string(),
        )
        .ok();

    let files = walk_workspace(root);
    if opts.verbose {
        eprintln!("project-map: {} candidate files", files.len());
    }
    if opts.progress {
        emit_progress("scan", 0, files.len());
    }

    // defs[name] = list of (node_id, lang_group) — language-scoped to prevent cross-language false edges
    // refs_per_file[file_node_id] = set of names referenced
    // file_lang[file_node_id] = lang_group for resolving refs against same-language defs
    let mut defs: HashMap<String, Vec<(i64, &'static str)>> = HashMap::new();
    let mut refs_per_file: HashMap<i64, HashSet<String>> = HashMap::new();
    let mut file_lang: HashMap<i64, &'static str> = HashMap::new();
    let mut report = IndexReport::default();
    report.files = files.len();

    let total = files.len();
    let mut last_progress: usize = 0;
    for (idx, wf) in files.iter().enumerate() {
        match index_file(&store, wf, opts.force, &mut defs, &mut refs_per_file, &mut file_lang) {
            Ok(IndexFileResult::Cached) => report.files_cached += 1,
            Ok(IndexFileResult::Indexed) => report.files_indexed += 1,
            Err(e) => {
                report.errors += 1;
                let fid = store.file_id(&wf.rel_path).ok().flatten();
                store.record_error(fid, &e).ok();
                if opts.verbose {
                    eprintln!("  err {}: {}", wf.rel_path, e);
                }
            }
        }
        if opts.progress && (idx + 1 == total || idx - last_progress >= 10) {
            last_progress = idx;
            emit_progress("index", idx + 1, total);
        }
    }

    let graph_dirty = opts.force || report.files_indexed > 0;
    if graph_dirty {
        if opts.progress { emit_progress("graph", 0, 1); }
        derive_edges(&store, &defs, &refs_per_file, &file_lang, &mut report)?;
        derive_bridge_edges(&store, &mut report)?;
        derive_http_bridge_edges(&store, &mut report)?;
        if let Err(e) = crate::cmd::map::scip_loader::load_workspace(root, &store) {
            eprintln!("project-map: scip loader: {e}");
        }
        rank_files(&store)?;
        if let Err(e) = store.rebuild_fts() {
            eprintln!("project-map: rebuild fts: {e}");
        }
        if opts.progress { emit_progress("graph", 1, 1); }
    }
    embed_pending_chunks(&store, &mut report, opts.progress);
    if opts.progress { emit_progress("done", 1, 1); }

    let counts = store.counts().map_err(|e| format!("counts: {e}"))?;
    report.nodes = counts.nodes as usize;
    report.edges = counts.edges as usize;
    report.errors = counts.errors as usize;
    report.elapsed_ms = started.elapsed().as_millis();

    store
        .set_meta(
            "last_index_finished_at",
            &chrono::Utc::now().timestamp_millis().to_string(),
        )
        .ok();
    Ok(report)
}

enum IndexFileResult {
    Cached,
    Indexed,
}

fn index_file(
    store: &Store,
    wf: &WorkspaceFile,
    force: bool,
    defs: &mut HashMap<String, Vec<(i64, &'static str)>>,
    refs_per_file: &mut HashMap<i64, HashSet<String>>,
    file_lang: &mut HashMap<i64, &'static str>,
) -> Result<IndexFileResult, String> {
    let mtime = fs_mtime_ns(&wf.abs_path).map_err(|e| format!("mtime: {e}"))?;
    let size = fs_size(&wf.abs_path).map_err(|e| format!("size: {e}"))?;

    if !force {
        if let Ok(check) = check_file(store, &wf.rel_path, mtime) {
            if check.fresh {
                hydrate_cached_file(store, wf, defs, refs_per_file, file_lang)?;
                return Ok(IndexFileResult::Cached);
            }
        }
    }

    let source = std::fs::read_to_string(&wf.abs_path).map_err(|e| format!("read: {e}"))?;
    if source.is_empty() {
        return Ok(IndexFileResult::Indexed);
    }

    let tags = extract(wf.lang, &source).map_err(|e| format!("extract: {e}"))?;

    let file_id = store
        .upsert_file(&wf.rel_path, Some(wf.lang.name()), mtime, size)
        .map_err(|e| format!("upsert file: {e}"))?;
    store
        .delete_file_data(file_id)
        .map_err(|e| format!("clear file data: {e}"))?;

    let file_node_serial = format!("file:{}", wf.rel_path);
    let file_node_display = file_basename(&wf.rel_path);
    let file_node_id = store
        .insert_node(&NewNode {
            kind: "file",
            serialized_name: &file_node_serial,
            display_name: &file_node_display,
            file_id: Some(file_id),
            start_line: None,
            start_col: None,
            end_line: None,
            end_col: None,
            metadata: None,
        })
        .map_err(|e| format!("insert file node: {e}"))?;

    let mut def_names_in_file: HashSet<String> = HashSet::new();
    file_lang.insert(file_node_id, wf.lang.resolution_group());

    for t in &tags {
        if t.role == TagRole::Def {
            insert_def(store, wf, t, file_id, file_node_id, defs, &mut def_names_in_file)?;
        }
    }

    let entry = refs_per_file.entry(file_node_id).or_default();
    for t in &tags {
        if t.role == TagRole::Ref && !def_names_in_file.contains(&t.name) {
            entry.insert(t.name.clone());
            store
                .insert_occurrence(file_id, None, &t.name, TagRole::Ref.as_str(), t.line as i64, t.col as i64)
                .map_err(|e| format!("insert occurrence: {e}"))?;
        }
    }

    record_bridge_signals(store, wf, file_id, &source)?;
    record_chunks(store, wf, file_id, &source)?;

    Ok(IndexFileResult::Indexed)
}

fn record_chunks(
    store: &Store,
    wf: &WorkspaceFile,
    file_id: i64,
    source: &str,
) -> Result<(), String> {
    for c in chunk_file(wf.lang, source) {
        let embed_text = build_embed_text(Some(&wf.rel_path), &c.text);
        let hash = hash_embed_text(&embed_text);
        let chunk_id = store
            .insert_chunk(file_id, c.start_line, c.end_line, &c.text, &hash)
            .map_err(|e| format!("insert chunk: {e}"))?;
        match store.lookup_embedding_cache(&hash) {
            Ok(Some(bytes)) => {
                if let Err(e) = store.store_chunk_vector(chunk_id, &bytes) {
                    eprintln!("project-map: replay vec for chunk {chunk_id}: {e}");
                }
            }
            Ok(None) => {}
            Err(e) => eprintln!("project-map: cache lookup {chunk_id}: {e}"),
        }
    }
    Ok(())
}

pub fn hash_embed_text(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let bytes = hasher.finalize();
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes.iter() {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn record_bridge_signals(
    store: &Store,
    wf: &WorkspaceFile,
    file_id: i64,
    source: &str,
) -> Result<(), String> {
    match wf.lang {
        Lang::Rust => {
            for h in handlers_in_rust(source) {
                store
                    .insert_bridge_signal(file_id, "handler", &h.name, h.line as i64)
                    .map_err(|e| format!("insert bridge handler: {e}"))?;
            }
        }
        Lang::TypeScript | Lang::Tsx | Lang::JavaScript => {
            for i in invokes_in_ts(source) {
                store
                    .insert_bridge_signal(file_id, "invoke", &i.command, i.line as i64)
                    .map_err(|e| format!("insert bridge invoke: {e}"))?;
            }
        }
        _ => {}
    }
    for h in extract_handlers(wf.lang, source) {
        store
            .insert_http_signal(file_id, "handler", &h.method, &h.path, h.line as i64)
            .map_err(|e| format!("insert http handler: {e}"))?;
    }
    for c in extract_calls(wf.lang, source) {
        store
            .insert_http_signal(file_id, "call", &c.method, &c.path, c.line as i64)
            .map_err(|e| format!("insert http call: {e}"))?;
    }
    Ok(())
}

fn insert_def(
    store: &Store,
    wf: &WorkspaceFile,
    t: &Tag,
    file_id: i64,
    file_node_id: i64,
    defs: &mut HashMap<String, Vec<(i64, &'static str)>>,
    def_names_in_file: &mut HashSet<String>,
) -> Result<(), String> {
    let serial = format!("{}:{}::{}#{}:{}", wf.lang.name(), wf.rel_path, t.name, t.line, t.col);
    let node_id = store
        .insert_node(&NewNode {
            kind: t.kind.as_str(),
            serialized_name: &serial,
            display_name: &t.name,
            file_id: Some(file_id),
            start_line: Some(t.line as i64),
            start_col: Some(t.col as i64),
            end_line: Some(t.end_line as i64),
            end_col: Some(t.end_col as i64),
            metadata: None,
        })
        .map_err(|e| format!("insert node: {e}"))?;

    store
        .insert_edge("contain", file_node_id, node_id, 1.0)
        .map_err(|e| format!("insert contain edge: {e}"))?;
    store
        .insert_occurrence(file_id, Some(node_id), &t.name, TagRole::Def.as_str(), t.line as i64, t.col as i64)
        .map_err(|e| format!("insert occurrence: {e}"))?;

    defs.entry(t.name.clone()).or_default().push((node_id, wf.lang.resolution_group()));
    def_names_in_file.insert(t.name.clone());
    Ok(())
}

fn hydrate_cached_file(
    store: &Store,
    wf: &WorkspaceFile,
    defs: &mut HashMap<String, Vec<(i64, &'static str)>>,
    refs_per_file: &mut HashMap<i64, HashSet<String>>,
    file_lang: &mut HashMap<i64, &'static str>,
) -> Result<(), String> {
    let file_node_serial = format!("file:{}", wf.rel_path);
    let file_node_id: Option<i64> = store
        .conn
        .query_row(
            "SELECT id FROM node WHERE serialized_name = ?1",
            rusqlite::params![file_node_serial],
            |r| r.get(0),
        )
        .ok();
    let Some(fnid) = file_node_id else { return Ok(()) };

    let file_id: Option<i64> = store
        .conn
        .query_row(
            "SELECT file_id FROM node WHERE id = ?1",
            rusqlite::params![fnid],
            |r| r.get(0),
        )
        .ok();
    let Some(fid) = file_id else { return Ok(()) };

    file_lang.insert(fnid, wf.lang.resolution_group());

    let mut stmt = store
        .conn
        .prepare("SELECT symbol_name, role FROM occurrence WHERE file_id = ?1")
        .map_err(|e| format!("prepare hydrate: {e}"))?;
    let rows = stmt
        .query_map(rusqlite::params![fid], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(|e| format!("hydrate query: {e}"))?;
    let entry = refs_per_file.entry(fnid).or_default();
    for row in rows.flatten() {
        if row.1 == "ref" {
            entry.insert(row.0);
        }
    }

    let mut stmt2 = store
        .conn
        .prepare("SELECT id, display_name FROM node WHERE file_id = ?1 AND kind != 'file'")
        .map_err(|e| format!("prepare hydrate defs: {e}"))?;
    let rows2 = stmt2
        .query_map(rusqlite::params![fid], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(|e| format!("hydrate defs: {e}"))?;
    for row in rows2.flatten() {
        defs.entry(row.1).or_default().push((row.0, wf.lang.resolution_group()));
    }
    Ok(())
}

fn derive_edges(
    store: &Store,
    defs: &HashMap<String, Vec<(i64, &'static str)>>,
    refs_per_file: &HashMap<i64, HashSet<String>>,
    file_lang: &HashMap<i64, &'static str>,
    report: &mut IndexReport,
) -> Result<(), String> {
    store
        .conn
        .execute("DELETE FROM edge WHERE kind = 'invoke'", [])
        .map_err(|e| format!("clear invoke edges: {e}"))?;

    let empty: HashSet<String> = HashSet::new();
    for (file_node_id, names) in refs_per_file {
        let Some(src_group) = file_lang.get(file_node_id).copied() else { continue };
        for name in names {
            if name.len() < 3 || is_too_common(name) {
                continue;
            }
            let Some(targets) = defs.get(name) else { continue };
            let same_lang_count = targets.iter().filter(|(_, g)| *g == src_group).count();
            for (tgt, tgt_group) in targets {
                if *tgt_group != src_group { continue; }
                if tgt == file_node_id { continue; }
                let weight = weight_for(name, same_lang_count, &empty);
                store
                    .insert_edge("invoke", *file_node_id, *tgt, weight)
                    .map_err(|e| format!("insert invoke edge: {e}"))?;
                report.edges += 1;
            }
        }
    }
    Ok(())
}

fn rank_files(store: &Store) -> Result<(), String> {
    let nodes = store
        .collect_all_node_ids()
        .map_err(|e| format!("collect nodes: {e}"))?;
    if nodes.is_empty() {
        return Ok(());
    }
    let edges_raw = store
        .collect_ranking_edges()
        .map_err(|e| format!("collect edges: {e}"))?;
    let edges: Vec<EdgeWeight> = edges_raw
        .into_iter()
        .map(|(s, t, w)| EdgeWeight { source: s, target: t, weight: w })
        .collect();
    let ranks = pagerank(&nodes, &edges, None);
    for (id, score) in ranks {
        store
            .update_node_importance(id, score)
            .map_err(|e| format!("update importance: {e}"))?;
    }
    Ok(())
}

fn emit_progress(phase: &str, done: usize, total: usize) {
    eprintln!("__progress__ {{\"phase\":\"{}\",\"done\":{},\"total\":{}}}", phase, done, total);
}

fn build_embed_text(path: Option<&str>, contents: &str) -> String {
    match path {
        Some(p) if !p.is_empty() => {
            let mut out = String::with_capacity(p.len() + contents.len() + 2);
            out.push_str(p);
            out.push('\n');
            out.push_str(contents);
            out
        }
        _ => contents.to_string(),
    }
}

fn embed_pending_chunks(store: &Store, report: &mut IndexReport, progress: bool) {
    let pending = match store.unembedded_chunks() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("project-map: list pending chunks: {e}");
            return;
        }
    };
    let total = pending.len();
    if total == 0 {
        if let Ok((tot, emb)) = store.count_chunks() {
            report.chunks = tot as usize;
            report.chunks_embedded = emb as usize;
        }
        return;
    }
    let mut client = EmbedClient::new();
    if let Err(e) = client.embed("warmup") {
        eprintln!("project-map: embed unavailable, skipping vectors ({e})");
        if let Ok((tot, emb)) = store.count_chunks() {
            report.chunks = tot as usize;
            report.chunks_embedded = emb as usize;
        }
        return;
    }

    let mut embedded = 0usize;
    let mut to_embed: Vec<(i64, String, String)> = Vec::new();
    for ch in pending {
        let prefixed = build_embed_text(ch.path.as_deref(), &ch.contents);
        let hash = hash_embed_text(&prefixed);
        if let Ok(Some(bytes)) = store.lookup_embedding_cache(&hash) {
            if let Err(e) = store.store_chunk_vector(ch.chunk_id, &bytes) {
                eprintln!("project-map: replay vec for {}: {e}", ch.chunk_id);
            } else {
                embedded += 1;
            }
            continue;
        }
        to_embed.push((ch.chunk_id, hash, prefixed));
    }

    const BATCH: usize = 32;
    let total_to_embed = to_embed.len();
    let mut processed = 0usize;
    if progress { emit_progress("embed", 0, total_to_embed); }
    for batch in to_embed.chunks(BATCH) {
        let texts: Vec<String> = batch.iter().map(|(_, _, t)| t.clone()).collect();
        match client.embed_batch(&texts) {
            Ok(vs) => {
                for ((chunk_id, hash, _), v) in batch.iter().zip(vs.iter()) {
                    if v.len() != VECTOR_DIM {
                        eprintln!(
                            "project-map: chunk {chunk_id} unexpected dim {} (want {VECTOR_DIM})",
                            v.len()
                        );
                        continue;
                    }
                    let bytes = vec_to_le_bytes(v);
                    if let Err(e) = store.store_chunk_vector(*chunk_id, &bytes) {
                        eprintln!("project-map: store vector {chunk_id}: {e}");
                        continue;
                    }
                    if let Err(e) = store.store_embedding_cache(hash, &bytes) {
                        eprintln!("project-map: cache store: {e}");
                    }
                    embedded += 1;
                }
            }
            Err(e) => {
                eprintln!("project-map: batch embed failed ({e}) — falling back to per-chunk");
                for (chunk_id, hash, prefixed) in batch.iter() {
                    match client.embed(prefixed) {
                        Ok(v) if v.len() == VECTOR_DIM => {
                            let bytes = vec_to_le_bytes(&v);
                            if let Err(e) = store.store_chunk_vector(*chunk_id, &bytes) {
                                eprintln!("project-map: store vector {chunk_id}: {e}");
                                continue;
                            }
                            if let Err(e) = store.store_embedding_cache(hash, &bytes) {
                                eprintln!("project-map: cache store: {e}");
                            }
                            embedded += 1;
                        }
                        Ok(v) => eprintln!(
                            "project-map: chunk {chunk_id} unexpected dim {} (want {VECTOR_DIM})",
                            v.len()
                        ),
                        Err(e) => eprintln!("project-map: embed chunk {chunk_id}: {e}"),
                    }
                }
            }
        }
        processed += batch.len();
        if progress { emit_progress("embed", processed, total_to_embed); }
    }
    if let Ok((tot, emb)) = store.count_chunks() {
        report.chunks = tot as usize;
        report.chunks_embedded = emb as usize;
    } else {
        report.chunks = total;
        report.chunks_embedded = embedded;
    }
}

fn derive_http_bridge_edges(store: &Store, report: &mut IndexReport) -> Result<(), String> {
    let signals = store
        .collect_http_signals()
        .map_err(|e| format!("collect http signals: {e}"))?;
    if signals.is_empty() {
        return Ok(());
    }
    let mut handlers: Vec<(i64, String, String)> = Vec::new();
    let mut calls: Vec<(i64, String, String)> = Vec::new();
    for (file_id, side, method, path, _line) in signals {
        match side.as_str() {
            "handler" => handlers.push((file_id, method, path)),
            "call" => calls.push((file_id, method, path)),
            _ => {}
        }
    }
    let mut emitted = 0usize;
    for (call_file, call_method, call_path) in &calls {
        let Some(call_node) = lookup_file_node(store, *call_file)? else { continue };
        for (h_file, h_method, h_path) in &handlers {
            if h_method != "*" && h_method != call_method {
                continue;
            }
            if !match_path(h_path, call_path) {
                continue;
            }
            let Some(h_node) = lookup_file_node(store, *h_file)? else { continue };
            if h_node == call_node {
                continue;
            }
            let metadata = format!(
                r#"{{"via":"http","method":"{}","path":"{}"}}"#,
                call_method, h_path
            );
            store
                .insert_edge_with_metadata("bridge", call_node, h_node, 1.0, Some(&metadata))
                .map_err(|e| format!("insert http bridge edge: {e}"))?;
            emitted += 1;
        }
    }
    report.edges += emitted;
    Ok(())
}

fn lookup_file_node(store: &Store, file_id: i64) -> Result<Option<i64>, String> {
    store
        .conn
        .query_row(
            "SELECT id FROM node WHERE file_id = ?1 AND kind = 'file'",
            rusqlite::params![file_id],
            |r| r.get::<_, i64>(0),
        )
        .ok()
        .map(Some)
        .map(Ok)
        .unwrap_or(Ok(None))
}

fn derive_bridge_edges(store: &Store, report: &mut IndexReport) -> Result<(), String> {
    store
        .conn
        .execute("DELETE FROM edge WHERE kind = 'bridge'", [])
        .map_err(|e| format!("clear bridge edges: {e}"))?;

    let signals = store
        .collect_bridge_signals()
        .map_err(|e| format!("collect signals: {e}"))?;
    if signals.is_empty() {
        return Ok(());
    }

    let mut handlers: HashMap<String, Vec<i64>> = HashMap::new();
    let mut invokes: HashMap<String, Vec<i64>> = HashMap::new();

    for (file_id, side, command, _line) in signals {
        let file_node_id: Option<i64> = store
            .conn
            .query_row(
                "SELECT id FROM node WHERE file_id = ?1 AND kind = 'file'",
                rusqlite::params![file_id],
                |r| r.get(0),
            )
            .ok();
        let Some(fnid) = file_node_id else { continue };
        match side.as_str() {
            "handler" => {
                let target_node: Option<i64> = store
                    .conn
                    .query_row(
                        "SELECT n.id FROM node n WHERE n.file_id = ?1 AND n.display_name = ?2 \
                         AND n.kind IN ('function','method') ORDER BY n.id LIMIT 1",
                        rusqlite::params![file_id, command],
                        |r| r.get(0),
                    )
                    .ok();
                handlers.entry(command).or_default().push(target_node.unwrap_or(fnid));
            }
            "invoke" => {
                invokes.entry(command).or_default().push(fnid);
            }
            _ => {}
        }
    }

    for (cmd, sources) in &invokes {
        let Some(targets) = handlers.get(cmd) else { continue };
        for src_id in sources {
            for tgt_id in targets {
                if src_id == tgt_id {
                    continue;
                }
                let metadata = format!(r#"{{"via":"tauri","command":"{}"}}"#, cmd);
                store
                    .insert_edge_with_metadata("bridge", *src_id, *tgt_id, 1.0, Some(&metadata))
                    .map_err(|e| format!("insert bridge edge: {e}"))?;
                report.edges += 1;
            }
        }
    }
    Ok(())
}

fn weight_for(name: &str, def_count: usize, mentioned: &HashSet<String>) -> f64 {
    let mut w = 1.0;
    if name.starts_with('_') {
        w *= 0.1;
    }
    if name.len() >= 8 && has_structured_case(name) {
        w *= 10.0;
    } else if name.len() >= 5 && has_structured_case(name) {
        w *= 2.0;
    }
    if def_count > 5 {
        w *= 0.1;
    }
    if mentioned.contains(name) {
        w *= 10.0;
    }
    w
}

fn has_structured_case(s: &str) -> bool {
    if s.contains('_') || s.contains('-') {
        return true;
    }
    let has_upper = s.chars().any(|c| c.is_uppercase());
    let has_lower = s.chars().any(|c| c.is_lowercase());
    has_upper && has_lower
}

fn is_too_common(name: &str) -> bool {
    matches!(
        name,
        "self" | "this" | "super" | "true" | "false" | "None" | "null" | "undefined"
            | "new" | "len" | "get" | "set" | "init" | "main" | "run" | "ok"
    )
}

fn file_basename(rel_path: &str) -> String {
    rel_path.rsplit('/').next().unwrap_or(rel_path).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn end_to_end_indexes_rust_fixture() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "pub fn special_codeterm_helper() -> i32 { 42 }\n",
        )
        .unwrap();
        fs::write(
            root.join("src/main.rs"),
            "use crate::lib;\nfn main() { let _ = special_codeterm_helper(); }\n",
        )
        .unwrap();

        let report = run_index(root, IndexOptions { force: true, verbose: false, progress: false }).unwrap();
        assert!(report.files_indexed >= 2, "indexed: {}", report.files_indexed);
        assert!(report.nodes > 0);

        let store = Store::open(root).unwrap();
        let rows = store.search_nodes("special_codeterm_helper", None, 5).unwrap();
        assert!(!rows.is_empty(), "search should find helper");
    }

    #[test]
    fn tauri_bridge_edges_emitted() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("src-tauri/src")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src-tauri/src/lib.rs"),
            "#[tauri::command]\npub fn mux_create_workspace() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("src/App.tsx"),
            r#"import { invoke } from "@tauri-apps/api/core"; invoke('mux_create_workspace', { name: 'x' });"#,
        )
        .unwrap();

        let report = run_index(root, IndexOptions { force: true, verbose: false, progress: false }).unwrap();
        assert!(report.files_indexed >= 2);

        let q = crate::cmd::map::graph::GraphQuery::open(root).unwrap();
        let g = q.full_graph(crate::cmd::map::graph::DEFAULT_NODE_CAP).unwrap();
        let bridges: Vec<_> = g.edges.iter().filter(|e| e.kind == "bridge").collect();
        assert!(!bridges.is_empty(), "expected at least 1 bridge edge, got {:?}", g.edges);
        let cmd = bridges[0]
            .metadata
            .as_ref()
            .and_then(|v| v.get("command").and_then(|c| c.as_str()))
            .unwrap_or("");
        assert_eq!(cmd, "mux_create_workspace");
    }

    #[test]
    fn cache_hit_on_second_run() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "fn alpha() {}\n").unwrap();

        let r1 = run_index(root, IndexOptions { force: false, verbose: false, progress: false }).unwrap();
        let r2 = run_index(root, IndexOptions { force: false, verbose: false, progress: false }).unwrap();
        assert!(r1.files_indexed >= 1);
        assert_eq!(r2.files_indexed, 0, "second pass should hit cache");
        assert!(r2.files_cached >= 1);
    }
}
