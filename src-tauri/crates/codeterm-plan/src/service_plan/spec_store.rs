//! Spec storage v2 - one YAML file per spec, sections inline.
//!
//! Layout per project specs dir:
//!   specs/<sp_id>.yaml  the complete Spec, including section bodies
//!
//! No JSON, no frontmatter, no section body fan-out: a spec mutation writes
//! only the spec YAML, and write-if-changed skips no-op rewrites.

use crate::plan::Spec;
use std::io;
use std::path::{Path, PathBuf};

pub fn spec_yaml_path(specs_dir: &Path, spec_id: &str) -> PathBuf {
    specs_dir.join(format!("{spec_id}.yaml"))
}

/// Co-located owned spec: `<plan_dir>/spec.yaml`.
pub fn spec_yaml_path_in_plan(plan_dir: &Path) -> PathBuf {
    plan_dir.join("spec.yaml")
}

fn to_yaml<T: serde::Serialize>(value: &T) -> io::Result<String> {
    serde_yaml::to_string(value).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn from_yaml<T: serde::de::DeserializeOwned>(text: &str) -> io::Result<T> {
    serde_yaml::from_str(text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Write only if the on-disk content differs - the property that keeps
/// untouched files out of git and off the disk.
fn write_if_changed(path: &Path, content: &str) -> io::Result<bool> {
    if matches!(std::fs::read_to_string(path), Ok(existing) if existing == content) {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    super::atomic_write::write_atomic(path, content)?;
    Ok(true)
}

/// Persist an orphan spec to the global specs dir. write-if-changed semantics.
pub fn write_spec(specs_dir: &Path, spec: &Spec) -> io::Result<bool> {
    write_if_changed(&spec_yaml_path(specs_dir, &spec.id), &to_yaml(spec)?)
}

/// Write a spec to an explicit path (co-located `<plan>/spec.yaml` or custom).
pub fn write_spec_at(path: &Path, spec: &Spec) -> io::Result<bool> {
    write_if_changed(path, &to_yaml(spec)?)
}

/// Load a spec from its v2 YAML file at an explicit path. Never writes.
pub fn load_spec_from_path(path: &Path) -> io::Result<Spec> {
    from_yaml(&std::fs::read_to_string(path)?)
}

/// Load an orphan spec from the global specs dir.
pub fn load_spec(specs_dir: &Path, spec_id: &str) -> io::Result<Spec> {
    load_spec_from_path(&spec_yaml_path(specs_dir, spec_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{ElementCanvas, PlanStatus, Section, Spec};

    fn sample_spec(dir: &Path) -> Spec {
        let mut spec = Spec::new("Storage v2 spec".into(), dir.display().to_string());
        spec.id = "sp_test".into();
        spec.status = PlanStatus::Active;
        spec.created_at = 1;
        spec.updated_at = 2;
        spec.sections = vec![Section {
            id: "sec_1".into(),
            title: "First section".into(),
            handle: Some("first-section".into()),
            tags: vec!["ux".into(), "backend".into()],
            body: "A spec body with\nmultiple lines\nas a block scalar.".into(),
            subsections: vec![Section {
                id: "sec_1_1".into(),
                title: "Nested section".into(),
                handle: Some("nested-section".into()),
                tags: vec!["nested".into()],
                body: "Nested body".into(),
                subsections: vec![],
                canvas: ElementCanvas {
                    x: Some(10.0),
                    y: Some(20.0),
                    collapsed: true,
                },
                attachments: vec![],
            }],
            canvas: ElementCanvas::default(),
            attachments: vec![],
        }];
        spec
    }

    #[test]
    fn spec_round_trips_through_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let spec = sample_spec(dir.path());
        write_spec(dir.path(), &spec).unwrap();
        let loaded = load_spec(dir.path(), &spec.id).unwrap();
        assert_eq!(loaded, spec);
        assert_eq!(
            loaded.sections[0].body,
            "A spec body with\nmultiple lines\nas a block scalar."
        );
        assert_eq!(loaded.sections[0].subsections[0].body, "Nested body");
    }

    #[test]
    fn spec_yaml_holds_section_bodies_inline() {
        let dir = tempfile::tempdir().unwrap();
        let spec = sample_spec(dir.path());
        write_spec(dir.path(), &spec).unwrap();
        let yaml = std::fs::read_to_string(spec_yaml_path(dir.path(), &spec.id)).unwrap();
        assert!(yaml.contains("First section"));
        assert!(yaml.contains("A spec body with"));
        assert!(yaml.contains("Nested body"));
        assert!(!dir.path().join(&spec.id).join("sections").exists());
    }

    #[test]
    fn identical_write_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let spec = sample_spec(dir.path());
        assert!(
            write_spec(dir.path(), &spec).unwrap(),
            "first write must create yaml"
        );
        assert!(
            !write_spec(dir.path(), &spec).unwrap(),
            "no-op write must skip"
        );
    }

    #[test]
    fn changed_spec_rewrites_its_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let mut spec = sample_spec(dir.path());
        write_spec(dir.path(), &spec).unwrap();
        spec.sections[0].body.push_str("\nNew detail.");
        assert!(
            write_spec(dir.path(), &spec).unwrap(),
            "changed spec must write"
        );
        let loaded = load_spec(dir.path(), &spec.id).unwrap();
        assert_eq!(loaded.sections[0].body, spec.sections[0].body);
    }
}
