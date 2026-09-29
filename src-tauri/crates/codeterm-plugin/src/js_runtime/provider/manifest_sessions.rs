//! Manifest-driven session discovery for plugins that declare `sessions.*`
//! without exporting `enumerateSessions` / `findSession`.

use std::path::{Component, Path, PathBuf};

use serde_json::{json, Value};

use super::super::PluginManifest;
use super::wire::{encode_cwd_dashed, session_description_head};
use super::JsProvider;
use crate::provider::{self, SessionMetadata};

fn is_session_stem(stem: &str) -> bool {
    stem.len() >= 32 && stem.contains('-')
}

impl JsProvider {
    pub(super) fn manifest_enumerate_sessions(&self) -> Vec<SessionMetadata> {
        let Some((root, dot_ext)) = self.manifest_session_store() else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir(&root) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for entry in entries.flatten() {
            if !entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                continue;
            }
            let Ok(files) = std::fs::read_dir(entry.path()) else {
                continue;
            };
            for f in files.flatten() {
                let name = f.file_name().to_string_lossy().to_string();
                let Some(stem) = name.strip_suffix(&dot_ext) else {
                    continue;
                };
                if is_session_stem(stem) {
                    out.push(self.manifest_session_metadata(stem, &f.path()));
                }
            }
        }
        out
    }

    pub(super) fn manifest_enumerate_sessions_for_search(&self) -> Vec<SessionMetadata> {
        let Some((root, dot_ext)) = self.manifest_session_store() else {
            return Vec::new();
        };
        let config = self
            .manifest
            .sessions
            .as_ref()
            .and_then(|sessions| sessions.search_files.as_ref());
        let search_root = match config.and_then(|config| config.root_subdir.as_deref()) {
            Some(subdir) => {
                let subdir = Path::new(subdir);
                if !subdir
                    .components()
                    .all(|part| matches!(part, Component::Normal(_)))
                {
                    return Vec::new();
                }
                root.join(subdir)
            }
            None => root,
        };
        let directory_depth = config
            .map(|config| {
                config
                    .directory_depth
                    .min(MAX_SESSION_SEARCH_DIRECTORY_DEPTH)
            })
            .unwrap_or(1);
        let file_stem_regex = match config.and_then(|config| config.file_stem_regex.as_deref()) {
            Some(pattern) => match regex::Regex::new(pattern) {
                Ok(regex) => Some(regex),
                Err(_) => return Vec::new(),
            },
            None => None,
        };
        let mut files = Vec::new();
        collect_session_files(&search_root, directory_depth, &mut files);
        let mut sessions: Vec<_> = files
            .into_iter()
            .filter_map(|path| {
                let name = path.file_name()?.to_string_lossy();
                let stem = name.strip_suffix(&dot_ext)?;
                let session_id = match file_stem_regex.as_ref() {
                    Some(regex) => regex.captures(stem)?.get(1)?.as_str().to_string(),
                    None if is_session_stem(stem) => stem.to_string(),
                    None => return None,
                };
                let metadata = std::fs::metadata(&path).ok()?;
                Some(SessionMetadata {
                    session_id: session_id.clone(),
                    provider: self.manifest.id.clone(),
                    project_path: None,
                    title: None,
                    summary: None,
                    file_path: path.to_string_lossy().to_string(),
                    transcript_path: None,
                    created_ts: provider::fs_time_to_unix(metadata.created().ok()),
                    modified_ts: provider::fs_time_to_unix(metadata.modified().ok()),
                })
            })
            .collect();
        sessions.sort_by_key(|session| std::cmp::Reverse(session.modified_ts.unwrap_or(0)));
        sessions
    }

    /// Probes `<project>/<id>.<ext>` once per project dir, so the cost follows
    /// the project count and never the number of stored sessions.
    pub(super) fn manifest_find_session(&self, session_id: &str) -> Option<SessionMetadata> {
        if !crate::session_evidence::valid_session_id(session_id) || !is_session_stem(session_id) {
            return None;
        }
        let (root, dot_ext) = self.manifest_session_store()?;
        let file_name = format!("{session_id}{dot_ext}");
        std::fs::read_dir(&root)
            .ok()?
            .flatten()
            .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
            .map(|e| e.path().join(&file_name))
            .find(|path| path.is_file())
            .map(|path| self.manifest_session_metadata(session_id, &path))
    }

    fn manifest_session_store(&self) -> Option<(PathBuf, String)> {
        let cfg = self.manifest.sessions.as_ref()?;
        let root = PluginManifest::expand_home(&cfg.root_dir)?;
        Some((root, format!(".{}", cfg.file_extension)))
    }

    fn manifest_session_metadata(&self, stem: &str, file_path: &Path) -> SessionMetadata {
        let meta = std::fs::metadata(file_path).ok();
        let session = SessionMetadata {
            session_id: stem.to_string(),
            provider: self.manifest.id.clone(),
            project_path: None,
            title: None,
            summary: None,
            file_path: file_path.to_string_lossy().to_string(),
            transcript_path: crate::session_evidence::stored_transcript_path(stem)
                .map(|p| p.to_string_lossy().to_string()),
            created_ts: meta
                .as_ref()
                .and_then(|m| provider::fs_time_to_unix(m.created().ok())),
            modified_ts: meta
                .as_ref()
                .and_then(|m| provider::fs_time_to_unix(m.modified().ok())),
        };
        self.manifest_describe_session(&session)
    }

    pub(super) fn manifest_describe_session(&self, session: &SessionMetadata) -> SessionMetadata {
        let extra: Option<Value> = self.try_call(
            "describeSession",
            &[
                json!(&session.file_path),
                json!(session_description_head(Path::new(&session.file_path))),
            ],
        );
        let described = |field: &str| {
            extra
                .as_ref()
                .and_then(|value| value.get(field))
                .and_then(|value| value.as_str())
                .map(String::from)
        };
        SessionMetadata {
            project_path: described("projectPath").or_else(|| session.project_path.clone()),
            title: described("title").or_else(|| session.title.clone()),
            summary: described("summary").or_else(|| session.summary.clone()),
            ..session.clone()
        }
    }
}

const MAX_SESSION_SEARCH_DIRECTORY_DEPTH: usize = 8;

fn collect_session_files(root: &Path, directory_depth: usize, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if directory_depth == 0 {
            if file_type.is_file() {
                files.push(entry.path());
            }
        } else if file_type.is_dir() {
            collect_session_files(&entry.path(), directory_depth - 1, files);
        }
    }
}

impl JsProvider {
    pub(super) fn sessions_dir(&self, cwd: &str) -> Option<PathBuf> {
        let cfg = self.manifest.sessions.as_ref()?;
        let root = PluginManifest::expand_home(&cfg.root_dir)?;
        let leaf = match cfg.cwd_encoding {
            super::super::CwdEncoding::DashEncoded => {
                encode_cwd_dashed(&codeterm_paths::canonical_cwd(cwd))
            }
            super::super::CwdEncoding::None => cwd.to_string(),
        };
        Some(root.join(leaf))
    }

    pub(super) fn owned_session_file_path(&self, cwd: &str, session_id: &str) -> Option<PathBuf> {
        if let Some(path) =
            self.try_call::<Option<String>>("sessionFilePath", &[json!(cwd), json!(session_id)])
        {
            return path.map(PathBuf::from);
        }
        let cfg = self.manifest.sessions.as_ref()?;
        Some(
            self.sessions_dir(cwd)?
                .join(format!("{}.{}", session_id, cfg.file_extension)),
        )
    }
}
