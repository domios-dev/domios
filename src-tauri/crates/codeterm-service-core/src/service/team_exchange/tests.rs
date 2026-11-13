use std::path::{Path, PathBuf};

use codeterm_team_package::contracts::{TemplateDistillRequest, TemplateProvenance, TemplateRef};
use codeterm_team_package::{
    check_folder, check_yaml, CheckOptions, ModelPreference, Package, PackageFormat, PackageSkill,
    RoleKind, RoleStart, StartMode,
};
use codeterm_types::team::role_registration::{PinnedPackage, RoleFileRef, RoleRegistration};

use super::distill::{bake, source_identity, RUNBOOK_HEADING};
use super::library::{sorted_versions, Saved, TemplateLibrary};
use super::scrub::{scrub_package, ScrubContext, Scrubber, MACHINE, WORKSPACE};
use super::{
    builtin_packages, check_import, default_package, forms, run_pipeline, INVALID_PACKAGE,
};
use crate::service::team_template_payload::{knowledge_path, Collected};

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../codeterm-team-package/fixtures/corpus")
}

fn inline_reference() -> Package {
    let text = std::fs::read_to_string(corpus().join("valid/reference-inline.team.yaml"))
        .expect("fixture reads");
    check_yaml(&text, &CheckOptions::default())
        .package
        .expect("inline reference is valid")
}

fn folder_reference() -> Package {
    check_folder(&corpus().join("valid/reference"), &CheckOptions::default())
        .package
        .expect("folder reference is valid")
}

fn private_context() -> ScrubContext {
    ScrubContext {
        personal_roots: vec!["C:\\Users\\ada\\work\\desk".into(), "/home/ada".into()],
        machines: vec!["ada-laptop".into()],
    }
}

fn read_form(path: &Path) -> Vec<(String, Vec<u8>)> {
    if path.is_dir() {
        let mut out = Vec::new();
        let mut pending = vec![(path.to_path_buf(), String::new())];
        while let Some((dir, prefix)) = pending.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let relative = if prefix.is_empty() {
                    name
                } else {
                    format!("{prefix}/{name}")
                };
                if entry.path().is_dir() {
                    pending.push((entry.path(), relative));
                } else {
                    out.push((relative, std::fs::read(entry.path()).unwrap()));
                }
            }
        }
        out.sort();
        out
    } else {
        vec![(String::new(), std::fs::read(path).unwrap())]
    }
}

/// Export, import the written form, export again: the bytes must not move.
fn round_trip(package: Package, format: PackageFormat, name: &str) -> Vec<(String, Vec<u8>)> {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join(format!("first-{name}"));
    let second = dir.path().join(format!("second-{name}"));
    let context = private_context();
    run_pipeline(package, &context, format, Some(first.clone())).expect("first export");
    let imported = check_import(&first).expect("the export imports");
    run_pipeline(imported, &context, format, Some(second.clone())).expect("second export");
    let (before, after) = (read_form(&first), read_form(&second));
    assert_eq!(before, after, "{format:?} export-import-export moved bytes");
    before
}

#[test]
fn export_import_export_is_byte_identical_in_every_form() {
    let yaml = round_trip(inline_reference(), PackageFormat::Yaml, "desk.team.yaml");
    let text = String::from_utf8(yaml[0].1.clone()).unwrap();
    assert!(text.contains("derived_from:\n  name: com.example/research-desk\n  version: 2.0.3"));
    assert!(
        !text.contains('\t') && !text.contains('\r'),
        "LF only, no tabs"
    );
    round_trip(folder_reference(), PackageFormat::Team, "desk.team");
    round_trip(folder_reference(), PackageFormat::Folder, "desk");
    let custom = custom_manager_package();
    for (format, name) in [
        (PackageFormat::Yaml, "custom.team.yaml"),
        (PackageFormat::Team, "custom.team"),
        (PackageFormat::Folder, "custom"),
    ] {
        round_trip(custom.clone(), format, name);
    }
    for package in builtin_packages() {
        round_trip(package.clone(), PackageFormat::Team, "builtin.team");
        round_trip(package.clone(), PackageFormat::Yaml, "builtin.team.yaml");
    }
}

#[test]
fn archive_bytes_and_entry_order_are_deterministic() {
    let package = folder_reference();
    let first = package.to_archive().unwrap();
    assert_eq!(first, package.to_archive().unwrap());
    let names: Vec<String> = package
        .archive_entries()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(names[0], "team.yaml");
    let mut sorted = names[1..].to_vec();
    sorted.sort();
    assert_eq!(names[1..], sorted[..]);
}

#[test]
fn a_svg_icon_stays_inline_and_a_png_refuses_the_one_file_form() {
    let default = default_package().expect("builtin default").clone();
    let inline = forms::single_file(&default).expect("an SVG icon inlines");
    assert!(inline.entries.is_empty());
    let icon = inline.document.icon.clone().unwrap();
    let file = inline
        .document
        .files
        .iter()
        .find(|file| file.path == icon)
        .unwrap();
    assert!(file.content.as_deref().unwrap().contains("<svg"));
    assert!(inline
        .document
        .roles()
        .all(|(_, role)| role.instructions_file.is_none()));

    let mut png = folder_reference();
    png.document.icon = Some("assets/logo.png".into());
    let error = forms::single_file(&png)
        .err()
        .expect("a PNG cannot be inline");
    assert!(error.contains("--format team"), "{error}");
    let error = run_pipeline(png, &ScrubContext::default(), PackageFormat::Yaml, None)
        .err()
        .expect("the yaml export refuses");
    assert!(error.message.contains("--format team"));
}

#[test]
fn the_scrub_rewrites_private_bindings_and_keeps_the_prose() {
    let mut scrubber = Scrubber::new(&private_context());
    let prose =
        "Never print the token: ask the owner. The token budget is large. Review issue 1234567.";
    assert_eq!(scrubber.text(prose, "here"), prose);
    let text = "Build in C:\\Users\\ada\\work\\desk\\app and /home/ada/notes on ada-laptop.\nOPENAI_API_KEY=sk-live1234567890abcdefXYZ\nCall https://api.example.com/v1?token=abc1234567";
    let clean = scrubber.text(text, "the worker instructions");
    assert!(clean.contains(&format!(
        "Build in {WORKSPACE}\\app and {WORKSPACE}/notes on {MACHINE}."
    )));
    assert!(clean.contains("OPENAI_API_KEY=[secret OPENAI_API_KEY]"));
    assert!(!clean.contains("sk-live") && !clean.contains("token=abc"));
    assert_eq!(scrubber.report.paths_generalized, 2);
    assert_eq!(scrubber.report.machines_removed, 1);
    assert!(scrubber.report.secret_values_removed >= 2);
    assert_eq!(
        scrubber.report.secret_names,
        vec!["OPENAI_API_KEY".to_string()]
    );
    assert_eq!(
        scrubber.text(&clean, "again"),
        clean,
        "the scrub is idempotent"
    );
    assert_eq!(
        scrubber.text("AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMIK7MDENG", "aws"),
        "AWS_SECRET_ACCESS_KEY=[secret AWS_SECRET_ACCESS_KEY]"
    );
}

#[test]
fn a_scrubbed_package_asks_for_its_secrets_by_name_and_keeps_authors() {
    let mut package = inline_reference();
    package
        .document
        .roles
        .get_mut("checker")
        .unwrap()
        .instructions = Some(
        "Use GITHUB_TOKEN: ghp_abcdefghijklmnopqrstuvwxyz0123 from /home/ada/.config.\n".into(),
    );
    package.document.memory = vec![codeterm_team_package::PackageMemory {
        title: "Where the archive lives".into(),
        kind: codeterm_team_package::MemoryKind::Fact,
        body: "Kept at /home/ada/archive.".into(),
        tags: Vec::new(),
    }];
    let report = scrub_package(&mut package, &private_context());
    let checker = package.document.role("checker").unwrap();
    assert_eq!(
        checker.instructions.as_deref(),
        Some("Use GITHUB_TOKEN: [secret GITHUB_TOKEN] from {workspace}/.config.\n")
    );
    assert_eq!(
        package.document.memory[0].body,
        "Kept at {workspace}/archive."
    );
    assert!(package
        .document
        .secrets
        .iter()
        .any(|secret| secret.name == "GITHUB_TOKEN"));
    assert_eq!(report.secret_asks_added, 1);
    assert!(report.secret_names.contains(&"SEARCH_API_KEY".to_string()));
    assert_eq!(
        package.document.authors[0].email.as_deref(),
        Some("jane@example.com")
    );
    assert_eq!(package.document.license.as_deref(), Some("Apache-2.0"));
}

#[test]
fn malformed_or_unsafe_packages_never_reach_my_templates() {
    let invalid = corpus().join("invalid");
    for case in [
        "anchor-alias.team.yaml",
        "duplicate-key.team.yaml",
        "tab-indentation.team.yaml",
        "missing-file.team.yaml",
        "icon-png-inline.team.yaml",
        "icon-svg-script.team.yaml",
        "unknown-field.team.yaml",
        "plan-template.team.yaml",
        "unreferenced-entry",
        "old-shape.team.yaml",
    ] {
        let error = check_import(&invalid.join(case))
            .err()
            .unwrap_or_else(|| panic!("{case} imports"));
        assert_eq!(error.code, INVALID_PACKAGE, "{case}");
        assert!(!error.diagnostics.is_empty(), "{case}");
    }
    let dir = tempfile::tempdir().unwrap();
    let secret = dir.path().join("secret.team.yaml");
    let mut package = inline_reference();
    package.document.roles.get_mut("orchestrator").unwrap().instructions =
        Some("Deploy with GITHUB_TOKEN=ghp_abcdefghijklmnopqrstuvwxyz0123\n".into());
    std::fs::write(&secret, package.to_yaml()).unwrap();
    let error = check_import(&secret)
        .err()
        .expect("a secret value is refused");
    assert!(error.message.contains("secret"));
    let relative = check_import(Path::new("relative.team.yaml")).err().unwrap();
    assert!(relative.message.contains("absolute"));

    let huge = dir.path().join("huge.team.yaml");
    let mut big = inline_reference();
    big.document.roles.get_mut("orchestrator").unwrap().instructions =
        Some("x".repeat(3 * 1024 * 1024));
    std::fs::write(&huge, big.to_yaml()).unwrap();
    assert!(
        check_import(&huge).is_err(),
        "instructions beyond the string bound are refused"
    );
}

#[test]
fn many_scripted_skills_import_unverified_with_one_script_ask_each() {
    let mut package = inline_reference();
    package.document.skills.clear();
    for index in 0..40 {
        package
            .document
            .skills
            .push(codeterm_team_package::PackageSkill::Inline(
                codeterm_team_package::InlineSkill {
                    name: format!("tool-{index}"),
                    description: "Runs a helper.".into(),
                    content: format!(
                        "---\nname: tool-{index}\ndescription: Runs a helper.\n---\nRun it.\n"
                    ),
                    files: vec![codeterm_team_package::SkillFile {
                        path: "scripts/run.sh".into(),
                        content: "#!/bin/sh\necho ok\n".into(),
                    }],
                },
            ));
    }
    package
        .document
        .roles
        .get_mut("researcher")
        .unwrap()
        .skills
        .clear();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tools.team.yaml");
    std::fs::write(&path, package.to_yaml()).unwrap();
    let imported = check_import(&path).expect("scripted skills are not malformed");
    let scripts = codeterm_team_package::contracts::declared_asks(&imported)
        .into_iter()
        .filter(|ask| {
            matches!(
                ask,
                codeterm_team_package::contracts::TemplateAsk::Script { .. }
            )
        })
        .count();
    assert_eq!(scripts, 40);
}

#[test]
fn my_templates_keep_versions_refuse_rewrites_and_report_updates() {
    let dir = tempfile::tempdir().unwrap();
    // The library refuses linked ancestors; a temp folder may sit under one (macOS /var).
    let library = TemplateLibrary::new(dir.path().canonicalize().unwrap());
    let first = inline_reference();
    assert!(matches!(
        library.save(&first, TemplateProvenance::Imported).unwrap(),
        Saved::Created { update: None }
    ));
    assert_eq!(
        library.save(&first, TemplateProvenance::Imported).unwrap(),
        Saved::Unchanged
    );
    let mut rewritten = first.clone();
    rewritten.document.description = "Something else.".into();
    let error = library
        .save(&rewritten, TemplateProvenance::Imported)
        .unwrap_err();
    assert!(error.contains("bump the version"));

    let mut next = first.clone();
    next.document.version = "2.2.0".into();
    next.document
        .roles
        .get_mut("checker")
        .unwrap()
        .instructions = Some("Check twice.\n".into());
    next.document
        .memory
        .push(codeterm_team_package::PackageMemory {
            title: "Cite the filing year".into(),
            kind: codeterm_team_package::MemoryKind::Rule,
            body: "State the year of every figure.".into(),
            tags: Vec::new(),
        });
    let Saved::Created { update: Some(diff) } =
        library.save(&next, TemplateProvenance::Imported).unwrap()
    else {
        panic!("a new version reports an update");
    };
    assert_eq!(
        (diff.from_version.as_str(), diff.to_version.as_str()),
        ("2.1.0", "2.2.0")
    );
    assert_eq!(diff.roles_changed, vec!["checker".to_string()]);
    assert_eq!(
        diff.payload_added,
        vec![codeterm_team_package::contracts::TemplateDiffItem {
            id: format!("memory:{}", first.document.memory.len()),
            label: "Cite the filing year".into(),
        }],
        "the diff names an added item by its label"
    );
    assert!(diff.payload_removed.is_empty());
    assert_eq!(
        library.versions(&first.document.name),
        vec!["2.1.0", "2.2.0"]
    );
    let (loaded, provenance) = library.load(&first.document.name, Some("2.1.0")).unwrap();
    assert_eq!(
        loaded.to_yaml(),
        first.to_yaml(),
        "the pinned version keeps its meaning"
    );
    assert_eq!(provenance, TemplateProvenance::Imported);
    assert_eq!(library.list()[0].version, "2.2.0");

    let mut builtin = first.clone();
    builtin.document.name = "dev.domios/default".into();
    assert!(library.save(&builtin, TemplateProvenance::Saved).is_err());

    let removed = library
        .delete(&TemplateRef {
            name: first.document.name.clone(),
            version: None,
        })
        .unwrap();
    assert_eq!(removed.len(), 2);
    assert!(library.list().is_empty());
    assert_eq!(
        sorted_versions(vec!["1.10.0", "1.2.0", "1.2.0-rc.1"]),
        vec!["1.2.0-rc.1", "1.2.0", "1.10.0"]
    );
}

fn registration(kind: RoleKind, name: &str, model: Option<ModelPreference>) -> RoleRegistration {
    RoleRegistration {
        team_id: "team_1".into(),
        kind,
        name: name.into(),
        label: if name == "cartographer" {
            "Mapper".into()
        } else {
            name.into()
        },
        description: Some(format!("{name} role")),
        start: RoleStart::Mode(StartMode::OnDemand),
        package: PinnedPackage {
            name: "dev.domios/default".into(),
            version: "1.0.0".into(),
        },
        instructions: format!("Do the {name} work.\n"),
        model,
        reasoning: None,
        placement_peer: Some("ada-laptop".into()),
        consent_decision_id: None,
        files: vec![RoleFileRef {
            path: knowledge_path("team_1", "notes/brief.md"),
            description: Some("Brief".into()),
        }],
        skills: Vec::new(),
        registered_at: 1,
    }
}

fn distill_request(runbook: Option<&str>) -> TemplateDistillRequest {
    serde_json::from_value(serde_json::json!({
        "team_id": "team_1",
        "name": "ada/desk",
        "version": "1.0.0",
        "description": "Runs the desk.",
        "license": "MIT",
        "runbook": runbook,
    }))
    .unwrap()
}

#[test]
fn bake_keeps_registered_roles_model_omission_runbook_and_lineage() {
    let base = default_package().unwrap().clone();
    let registrations = vec![
        registration(RoleKind::Manager, "orchestrator", None),
        registration(RoleKind::Manager, "cartographer", None),
        registration(RoleKind::Manager, "security_lead", None),
        registration(
            RoleKind::Worker,
            "analyst",
            Some(ModelPreference::List(vec!["claude:opus".into()])),
        ),
        registration(RoleKind::Worker, "scribe", None),
    ];
    let collected = Collected {
        files: vec![codeterm_team_package::PackageFile {
            path: "notes/brief.md".into(),
            description: Some("Brief".into()),
            content: None,
        }],
        entries: [("notes/brief.md".to_string(), b"# Brief\n".to_vec())]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    let baked = bake(
        &distill_request(Some("Split by angle.")),
        &registrations,
        &base,
        collected,
    );
    let document = &baked.document;
    assert_eq!(document.name, "ada/desk");
    assert_eq!(document.license.as_deref(), Some("MIT"));
    let lineage = document.derived_from.as_ref().unwrap();
    assert_eq!(
        (lineage.name.as_str(), lineage.version.as_str()),
        ("dev.domios/default", "1.0.0")
    );
    let orchestrator = document
        .role("orchestrator")
        .and_then(|role| role.instructions.as_deref())
        .unwrap();
    assert!(orchestrator.starts_with("Do the orchestrator work.\n"));
    assert!(orchestrator.ends_with(&format!("{RUNBOOK_HEADING}\n\nSplit by angle.\n")));
    let names: Vec<&str> = document.roles.keys().collect();
    assert_eq!(
        names,
        ["orchestrator", "cartographer", "security_lead", "analyst", "scribe"],
        "the bake carries exactly the registered roles, never-spawned ones included"
    );
    let security = document.manager("security_lead").unwrap();
    assert_eq!(
        security.instructions.as_deref(),
        Some("Do the security_lead work.\n")
    );
    let cartographer = document.manager("cartographer").unwrap();
    assert_eq!(cartographer.name.as_deref(), Some("Mapper"));
    assert_eq!(
        cartographer.instructions.as_deref(),
        Some("Do the cartographer work.\n")
    );
    let scribe = document.role("scribe").unwrap();
    assert_eq!(scribe.kind, RoleKind::Worker);
    assert!(scribe.model.is_none(), "model omission survives");
    assert_eq!(scribe.files, vec!["notes/brief.md".to_string()]);
    assert!(document.role("planner").is_none(), "a Team has exactly its roles");

    let exported = run_pipeline(baked, &private_context(), PackageFormat::Yaml, None)
        .expect("the bake checks");
    assert!(!exported.yaml.contains("ada-laptop"));
    assert!(exported.check.valid);

    let unstarted = bake(&distill_request(None), &[], &base, Collected::default());
    assert!(
        unstarted.document.derived_from.is_none(),
        "no template provenance, no lineage"
    );
    assert_eq!(unstarted.document.roles.len(), base.document.roles.len());
    assert!(unstarted
        .document
        .roles()
        .all(|(_, role)| role.instructions.is_some() && role.instructions_file.is_none()));
    assert_eq!(
        source_identity(&registrations).unwrap().name,
        "dev.domios/default"
    );
}

#[test]
fn the_template_list_carries_my_templates_and_their_persisted_trust() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let library = TemplateLibrary::new(&root);
    let package = inline_reference();
    let (name, version) = (&package.document.name, &package.document.version);
    library
        .save(&package, TemplateProvenance::Imported)
        .unwrap();
    assert!(
        library.mark_verified(name, "9.9.9").is_err(),
        "absent versions stay absent"
    );
    library.mark_received(name, version, "Friend-A").unwrap();
    library.mark_verified(name, version).unwrap();

    let restarted = TemplateLibrary::new(&root);
    let listed = crate::service::team_template_start::list_in(&restarted).templates;
    let builtins = crate::service::team_template_start::builtin_templates().count();
    assert!(listed[..builtins]
        .iter()
        .all(|summary| summary.provenance == TemplateProvenance::Builtin));
    let mine = &listed[builtins..];
    assert_eq!(mine.len(), 1);
    assert_eq!(&mine[0].name, name);
    assert_eq!(mine[0].received_from.as_deref(), Some("Friend-A"));
    assert!(mine[0].verified);
    assert_eq!(
        mine[0].workers as usize,
        package.document.roles_of(RoleKind::Worker).count()
    );
    // Preview and recall resolve the same package and must show the gallery's trust.
    let (resolved, provenance) = super::resolve_in(
        &restarted,
        &TemplateRef {
            name: name.clone(),
            version: None,
        },
    )
    .unwrap();
    assert_eq!(restarted.summarize(&resolved, provenance), mine[0]);
    assert_eq!(
        library
            .save(&package, TemplateProvenance::Imported)
            .unwrap(),
        Saved::Unchanged
    );
    assert!(
        restarted.summary(&package).verified,
        "a re-save keeps the record"
    );
}

#[test]
fn archive_forms_are_the_same_bytes_whatever_form_the_package_arrived_in() {
    let dir = tempfile::tempdir().unwrap();
    let context = private_context();
    let mut compared = 0;
    for (index, package) in builtin_packages().iter().enumerate() {
        let Ok(inline) = forms::single_file(package) else {
            continue;
        };
        let from_yaml = check_yaml(&inline.to_yaml(), &CheckOptions::default())
            .package
            .expect("the one-file form imports");
        for (format, suffix) in [
            (PackageFormat::Team, "team"),
            (PackageFormat::Folder, "dir"),
        ] {
            let original = dir.path().join(format!("{index}-original.{suffix}"));
            let imported = dir.path().join(format!("{index}-imported.{suffix}"));
            run_pipeline(package.clone(), &context, format, Some(original.clone())).unwrap();
            run_pipeline(from_yaml.clone(), &context, format, Some(imported.clone())).unwrap();
            assert_eq!(
                read_form(&original),
                read_form(&imported),
                "{} {format:?}",
                package.document.name
            );
        }
        compared += 1;
    }
    assert!(compared > 0, "at least one builtin has a one-file form");

    let canonical = forms::externalized(&inline_reference()).unwrap();
    assert!(canonical
        .document
        .files
        .iter()
        .all(|file| file.content.is_none()));
    assert!(canonical
        .document
        .roles()
        .all(|(_, role)| role.instructions.is_none()));
    assert!(canonical
        .document
        .skills
        .iter()
        .all(|skill| matches!(skill, PackageSkill::Path { path } if path.starts_with("skills/"))));
    assert_eq!(
        forms::externalized(&canonical).unwrap().archive_entries(),
        canonical.archive_entries(),
        "the canonical form is a fixed point"
    );
}

/// A package with a custom with_team manager and an on-demand one beside the built-ins.
fn custom_manager_package() -> Package {
    let text = std::fs::read_to_string(corpus().join("valid/custom-manager.team.yaml"))
        .expect("fixture reads");
    check_yaml(&text, &CheckOptions::default())
        .package
        .expect("custom manager fixture is valid")
}

#[test]
fn a_custom_manager_exports_with_its_kind_first_and_full_instructions() {
    let exported = run_pipeline(
        custom_manager_package(),
        &private_context(),
        PackageFormat::Yaml,
        None,
    )
    .expect("the custom manager package exports");
    assert!(exported.check.valid);
    let yaml = exported.yaml;
    assert!(yaml.contains("  security_lead:\n    kind: manager\n    name: Warden\n"));
    assert!(yaml.contains("Review every change for security risks"));
    assert!(!yaml.contains("managers:") && !yaml.contains("workers:"));
}
