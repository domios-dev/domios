use crate::cmd::map::store::Store;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct Architecture {
    pub modules: HashMap<String, ModuleSpec>,
    #[serde(default)]
    pub allowed_dependencies: Vec<DependencyRule>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModuleSpec {
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DependencyRule {
    pub from: String,
    pub to: Vec<String>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct FitnessReport {
    pub modules: usize,
    pub rules: usize,
    pub confirmed_edges: usize,
    pub violation_edges: Vec<Violation>,
    pub orphan_edges: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Violation {
    pub from: String,
    pub to: String,
    pub source_path: String,
    pub target_path: String,
    pub kind: String,
}

pub fn architecture_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".codeterm").join("map").join("v1").join("architecture.yml")
}

pub fn load(workspace_root: &Path) -> Result<Option<Architecture>, String> {
    let p = architecture_path(workspace_root);
    if !p.is_file() {
        return Ok(None);
    }
    let body = std::fs::read_to_string(&p).map_err(|e| format!("read: {e}"))?;
    let arch: Architecture = serde_yaml::from_str(&body).map_err(|e| format!("yaml: {e}"))?;
    Ok(Some(arch))
}

pub fn check(workspace_root: &Path) -> Result<FitnessReport, String> {
    let Some(arch) = load(workspace_root)? else {
        return Err(format!(
            "no architecture.yml at {}",
            architecture_path(workspace_root).display()
        ));
    };
    let store = Store::open(workspace_root).map_err(|e| format!("open store: {e}"))?;
    run_check(&store, &arch)
}

fn run_check(store: &Store, arch: &Architecture) -> Result<FitnessReport, String> {
    let mut report = FitnessReport::default();
    report.modules = arch.modules.len();
    report.rules = arch.allowed_dependencies.len();

    let allowed: HashMap<String, HashSet<String>> = arch
        .allowed_dependencies
        .iter()
        .map(|r| (r.from.clone(), r.to.iter().cloned().collect::<HashSet<String>>()))
        .collect();

    let path_to_module = build_path_index(arch);

    let edges = store
        .collect_ranking_edges()
        .map_err(|e| format!("collect edges: {e}"))?;

    for (src_id, tgt_id, _w) in edges {
        let Some(src_path) = node_path(store, src_id)? else { continue };
        let Some(tgt_path) = node_path(store, tgt_id)? else { continue };
        let Some(src_mod) = match_module(&src_path, &path_to_module) else { continue };
        let Some(tgt_mod) = match_module(&tgt_path, &path_to_module) else { continue };
        if src_mod == tgt_mod {
            continue;
        }
        match allowed.get(&src_mod) {
            Some(allowed_targets) if allowed_targets.contains(&tgt_mod) => {
                report.confirmed_edges += 1;
            }
            Some(_) => {
                report.violation_edges.push(Violation {
                    from: src_mod.clone(),
                    to: tgt_mod.clone(),
                    source_path: src_path,
                    target_path: tgt_path,
                    kind: "violation".into(),
                });
            }
            None => {
                report.orphan_edges += 1;
            }
        }
    }
    Ok(report)
}

fn build_path_index(arch: &Architecture) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (name, spec) in &arch.modules {
        for prefix in &spec.paths {
            out.push((name.clone(), prefix.clone()));
        }
    }
    out.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    out
}

fn match_module(path: &str, index: &[(String, String)]) -> Option<String> {
    for (module, prefix) in index {
        if path.starts_with(prefix) {
            return Some(module.clone());
        }
    }
    None
}

fn node_path(store: &Store, node_id: i64) -> Result<Option<String>, String> {
    let row: Option<String> = store
        .conn
        .query_row(
            "SELECT f.path FROM node n LEFT JOIN file f ON n.file_id = f.id WHERE n.id = ?1",
            rusqlite::params![node_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten();
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longest_prefix_wins() {
        let arch_yaml = r#"
modules:
  ui:
    paths: [src/]
  ui-buttons:
    paths: [src/components/buttons/]
allowed_dependencies: []
"#;
        let arch: Architecture = serde_yaml::from_str(arch_yaml).unwrap();
        let idx = build_path_index(&arch);
        assert_eq!(match_module("src/components/buttons/x.tsx", &idx), Some("ui-buttons".into()));
        assert_eq!(match_module("src/lib.ts", &idx), Some("ui".into()));
        assert_eq!(match_module("docs/x.md", &idx), None);
    }
}
