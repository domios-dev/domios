//! Manifest-driven session discovery for plugins that declare `sessions.*`
//! without exporting `enumerateSessions` / `findSession`.

use std::path::{Path, PathBuf};

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
        let extra: Option<Value> = self.try_call(
            "describeSession",
            &[
                json!(file_path.to_string_lossy()),
                json!(session_description_head(file_path)),
            ],
        );
        let described = |field: &str| {
            extra
                .as_ref()
                .and_then(|v| v.get(field))
                .and_then(|s| s.as_str())
                .map(String::from)
        };
        SessionMetadata {
            session_id: stem.to_string(),
            provider: self.manifest.id.clone(),
            project_path: described("projectPath"),
            title: described("title"),
            summary: described("summary"),
            file_path: file_path.to_string_lossy().to_string(),
            transcript_path: crate::session_evidence::stored_transcript_path(stem)
                .map(|p| p.to_string_lossy().to_string()),
            created_ts: meta
                .as_ref()
                .and_then(|m| provider::fs_time_to_unix(m.created().ok())),
            modified_ts: meta
                .as_ref()
                .and_then(|m| provider::fs_time_to_unix(m.modified().ok())),
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
