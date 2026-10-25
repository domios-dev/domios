use super::*;
use serde_json::json;
struct Memory(Document);
impl MemoryResolver for Memory {
    fn head(&self, id: &str) -> Result<Document, String> {
        if id == "mem:x" {
            Ok(self.0.clone())
        } else {
            Err("Outside scope".into())
        }
    }
    fn revision(&self, pin: &CallPin) -> Result<Document, String> {
        if pin.id == "mem:x" && document_projection::identity(&self.0)? == pin.hash {
            Ok(self.0.clone())
        } else {
            Err("Missing pinned hash".into())
        }
    }
}
fn leaf() -> Document {
    Document::from_definition(&json!({"schema_version":2,"kind":"pipeline","name":"leaf","inputs":{"n":{"type":"integer"}},"steps":[{"id":"increment","code":{"lang":"python","src":"def main(n): return n+1"},"in":{"type":"integer"},"out":{"type":"integer"}}],"result":{"type":{"type":"integer"},"from":"increment.out"}})).unwrap()
}
fn branch(reference: &str) -> Document {
    Document::from_definition(&json!({"schema_version":2,"kind":"pipeline","name":"branch","inputs":{"n":{"type":"integer"}},"steps":[{"id":"invoke","call":{"ref":reference,"with":{"n":{"input":"n"}}},"out":{"type":"integer"}}],"result":{"type":{"type":"integer"},"from":"invoke.out"}})).unwrap()
}
#[test]
fn blueprint_v2_call_local_bundle_seals_nested_mem_transitively() {
    let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("callee.yaml"), branch("mem:x").yaml().unwrap()).unwrap();
    let mut root = branch("local");
    root.imports.insert("local".into(), "callee.yaml".into());
    let bundle = collect_local(
        dir.join("root.yaml").to_str().unwrap(),
        &root.yaml().unwrap(),
    )
    .unwrap();
    assert!(bundle.document.steps[0]
        .call
        .as_ref()
        .unwrap()
        .pin
        .is_none());
    std::fs::remove_dir_all(dir).unwrap();
    let closure = resolve_bundle(bundle.document, &bundle.local_callees, &Memory(leaf())).unwrap();
    assert_eq!(closure.callees.len(), 2);
    let local = closure
        .callee(
            closure.root.document.steps[0]
                .call
                .as_ref()
                .unwrap()
                .pin
                .as_ref()
                .unwrap(),
        )
        .unwrap();
    let pin = local.document.steps[0]
        .call
        .as_ref()
        .unwrap()
        .pin
        .as_ref()
        .unwrap();
    assert_eq!(pin.id, "mem:x");
    assert_eq!(
        closure.callee(pin).unwrap().hash,
        document_projection::identity(&leaf()).unwrap()
    );
    assert_eq!(
        local.hash,
        document_projection::identity(&local.document).unwrap()
    );
}
#[test]
fn blueprint_v2_call_local_bundle_source_labels_are_inert() {
    for label in [
        "/absolute/path/that/must/not/be/opened.yaml",
        "../../private/unreadable.yaml",
    ] {
        let mut root = branch("local");
        root.imports.insert("local".into(), "local:body".into());
        let closure = resolve_bundle(
            root,
            &[LocalCallee {
                id: "local:body".into(),
                source_label: label.into(),
                document: leaf(),
            }],
            &Memory(leaf()),
        )
        .unwrap();
        assert_eq!(closure.callees.len(), 1);
        assert_eq!(
            closure.callees.values().next().unwrap().document.as_ref(),
            &leaf()
        );
    }
}
#[test]
fn blueprint_v2_call_single_checker_checks_callee_before_verification() {
    let mut root = branch("local");
    root.imports.insert("local".into(), "local:body".into());
    let mut invalid = leaf();
    invalid.steps[0]
        .code
        .as_mut()
        .unwrap()
        .caps
        .push("net".into());
    let closure = resolve_bundle(
        root,
        &[LocalCallee {
            id: "local:body".into(),
            source_label: "fixture".into(),
            document: invalid,
        }],
        &Memory(leaf()),
    )
    .unwrap();
    let context = check_context(&closure, &Default::default());
    assert!(!context.calls.contains_key("invoke"));
    assert!(context.call_diagnostics.iter().any(|d| d.rule == "C10"));
    assert!(crate::blueprint_document::checker::has_errors(
        &crate::blueprint_document::checker::check_ir(&closure.root.ir(), &context)
    ));
}

#[test]
fn blueprint_v2_call_local_code_file_is_frozen_relative_to_exact_callee_author() {
    let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(dir.join("nested")).unwrap();
    std::fs::write(dir.join("nested/body.py"), "def main(n): return n+1").unwrap();
    std::fs::write(dir.join("body.py"), "def main(n): return 99").unwrap();
    let mut callee = leaf();
    let code = callee.steps[0].code.as_mut().unwrap();
    code.src = None;
    code.file = Some("body.py".into());
    std::fs::write(dir.join("nested/callee.yaml"), callee.yaml().unwrap()).unwrap();
    let mut root = branch("local");
    root.imports
        .insert("local".into(), "nested/callee.yaml".into());
    let bundle = collect_local(
        dir.join("root.yaml").to_str().unwrap(),
        &root.yaml().unwrap(),
    )
    .unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    let closure = resolve_bundle(bundle.document, &bundle.local_callees, &Memory(leaf())).unwrap();
    let code = closure.callees.values().next().unwrap().document.steps[0]
        .code
        .as_ref()
        .unwrap();
    assert_eq!(code.src.as_deref(), Some("def main(n): return n+1"));
    assert!(code.file.is_none());
    let sealed = closure.document();
    let bundle = collect_local("no-local-files-left.yaml", &sealed.yaml().unwrap()).unwrap();
    assert!(bundle.local_callees.is_empty());
    assert_eq!(bundle.document, sealed);
}
#[test]
fn blueprint_v2_call_host_bundle_refuses_unsealed_code_files_without_opening_labels() {
    let mut root = branch("local");
    root.imports.insert("local".into(), "local:body".into());
    let mut callee = leaf();
    let code = callee.steps[0].code.as_mut().unwrap();
    code.src = None;
    code.file = Some("sensitive.py".into());
    let failure = resolve_bundle(
        root,
        &[LocalCallee {
            id: "local:body".into(),
            source_label: "/private/label/is/not/authority.yaml".into(),
            document: callee,
        }],
        &Memory(leaf()),
    )
    .unwrap_err();
    assert!(failure
        .iter()
        .any(|d| d.message.contains("frozen into inline source")));
}

#[test]
fn blueprint_v2_call_review_host_persists_safe_dependency_labels() {
    let mut root = branch("local");
    root.imports.insert("local".into(), "local:body".into());
    let closure = resolve_bundle(
        root,
        &[LocalCallee {
            id: "local:body".into(),
            source_label: "/Users/private-owner/author/leaf.yaml".into(),
            document: leaf(),
        }],
        &Memory(leaf()),
    )
    .unwrap();
    for location in closure
        .callees
        .values()
        .flat_map(|callee| callee.locations.values())
    {
        assert!(!location.file.contains("/Users/"), "{}", location.file);
        assert!(!std::path::Path::new(&location.file).is_absolute());
    }
}

#[cfg(windows)]
fn collect_code_file(
    dir: &std::path::Path,
    relative: &str,
) -> Result<LocalBundle, Vec<Diagnostic>> {
    let mut root = leaf();
    let code = root.steps[0].code.as_mut().unwrap();
    code.src = None;
    code.file = Some(relative.into());
    collect_local(
        dir.join("root.yaml").to_str().unwrap(),
        &root.yaml().unwrap(),
    )
}

#[cfg(windows)]
#[test]
fn blueprint_v2_code_file_freeze_refuses_junction_outside_apply_root() {
    let base = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
    let dir = base.join("apply");
    let outside = base.join("extlib");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("inc.py"), "def main(n): return n+100").unwrap();
    let link = dir.join("extj");
    let output = std::process::Command::new("cmd.exe")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(&link)
        .arg(&outside)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result = collect_code_file(&dir, "extj/inc.py");
    std::fs::remove_dir(&link).unwrap();
    std::fs::remove_dir_all(base).unwrap();
    assert!(result.unwrap_err()[0]
        .message
        .contains("contract_violation"));
}

#[test]
fn blueprint_v2_local_import_refuses_traversal_outside_importing_directory() {
    let base = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
    let dir = base.join("apply");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(base.join("callee.yaml"), leaf().yaml().unwrap()).unwrap();
    let mut root = branch("local");
    root.imports.insert("local".into(), "../callee.yaml".into());
    let path = dir.join("root.yaml");
    let source = root.yaml().unwrap();
    let bundled = collect_local(path.to_str().unwrap(), &source).unwrap_err();
    let resolved = resolve_local(path.to_str().unwrap(), &source).unwrap_err();
    std::fs::remove_dir_all(base).unwrap();
    assert!(bundled[0].message.contains("contract_violation"));
    assert!(resolved[0].message.contains("contract_violation"));
}

#[cfg(windows)]
#[test]
fn blueprint_v2_local_import_refuses_junction_outside_importing_directory() {
    let base = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
    let dir = base.join("apply");
    let outside = base.join("extlib");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("callee.yaml"), leaf().yaml().unwrap()).unwrap();
    let link = dir.join("extj");
    let output = std::process::Command::new("cmd.exe")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(&link)
        .arg(&outside)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut root = branch("local");
    root.imports
        .insert("local".into(), "extj/callee.yaml".into());
    let path = dir.join("root.yaml");
    let source = root.yaml().unwrap();
    let bundled = collect_local(path.to_str().unwrap(), &source);
    let resolved = resolve_local(path.to_str().unwrap(), &source);
    std::fs::remove_dir(&link).unwrap();
    std::fs::remove_dir_all(base).unwrap();
    assert!(bundled.unwrap_err()[0]
        .message
        .contains("contract_violation"));
    assert!(resolved.unwrap_err()[0]
        .message
        .contains("contract_violation"));
}
