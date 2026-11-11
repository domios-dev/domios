use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::archive::{read_folder, write_zip};
use crate::*;

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/corpus")
}

#[derive(Deserialize)]
struct Cases {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    path: String,
    valid: bool,
    #[serde(default)]
    canonical: bool,
    #[serde(default)]
    expect: Vec<Expect>,
}

#[derive(Deserialize)]
struct Expect {
    code: DiagnosticCode,
    path: String,
    line: Option<u32>,
}

type Signature = Vec<(String, String, String, String, Option<u32>)>;

fn signature(report: &PackageCheckReport) -> Signature {
    let mut out: Signature = report
        .diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.code.name(),
                format!("{:?}", diagnostic.severity),
                diagnostic.file.clone(),
                diagnostic.path.clone(),
                diagnostic.line,
            )
        })
        .collect();
    out.sort();
    out
}

fn single_entry(text: &[u8]) -> Vec<(String, Vec<u8>)> {
    vec![("team.yaml".to_string(), text.to_vec())]
}

/// The same case read in every form it can take.
fn forms(case: &str) -> Vec<(PackageFormat, Checked)> {
    let options = CheckOptions::domios();
    let full = corpus().join(case);
    if full.is_dir() {
        let mut diagnostics = Vec::new();
        let entries =
            read_folder(&full, &options.limits, &mut diagnostics).expect("fixture folder reads");
        assert!(diagnostics.is_empty(), "{case}: {diagnostics:?}");
        let zip = write_zip(&entries).expect("fixture zips");
        let mut out = vec![
            (PackageFormat::Folder, check_folder(&full, &options)),
            (PackageFormat::Team, check_archive(&zip, &options)),
        ];
        if entries.len() == 1 {
            let text = String::from_utf8(entries[0].1.clone()).expect("utf-8 team.yaml");
            out.push((PackageFormat::Yaml, check_yaml(&text, &options)));
        }
        out
    } else {
        let bytes = std::fs::read(&full).expect("fixture reads");
        let text = String::from_utf8(bytes.clone()).expect("utf-8 fixture");
        vec![
            (PackageFormat::Yaml, check_yaml(&text, &options)),
            (
                PackageFormat::Folder,
                check_entries(
                    PackageFormat::Folder,
                    single_entry(&bytes),
                    Vec::new(),
                    &options,
                ),
            ),
            (
                PackageFormat::Team,
                check_archive(&write_zip(&single_entry(&bytes)).expect("zips"), &options),
            ),
        ]
    }
}

fn root_text(case: &str) -> String {
    let full = corpus().join(case);
    let file = if full.is_dir() {
        full.join("team.yaml")
    } else {
        full
    };
    std::fs::read_to_string(file).expect("fixture reads")
}

#[test]
fn corpus_cases_report_the_same_codes_and_locations_in_every_form() {
    let cases: Cases = serde_json::from_str(
        &std::fs::read_to_string(corpus().join("cases.json")).expect("cases.json"),
    )
    .expect("cases.json parses");
    assert!(!cases.cases.is_empty());
    for case in &cases.cases {
        let results = forms(&case.path);
        let reference = signature(&results[0].1.report);
        for (format, checked) in &results {
            let report = &checked.report;
            assert_eq!(
                report.valid, case.valid,
                "{} as {format:?}: {:#?}",
                case.path, report.diagnostics
            );
            assert_eq!(report.format, *format);
            assert_eq!(
                signature(report),
                reference,
                "{} differs as {format:?}",
                case.path
            );
            assert_eq!(checked.package.is_some(), case.valid, "{}", case.path);
            for expected in &case.expect {
                assert!(
                    report
                        .diagnostics
                        .iter()
                        .any(|diagnostic| diagnostic.code == expected.code
                            && diagnostic.path == expected.path
                            && expected
                                .line
                                .is_none_or(|line| diagnostic.line == Some(line))),
                    "{} as {format:?} lacks {:?} at {} line {:?}: {:#?}",
                    case.path,
                    expected.code,
                    expected.path,
                    expected.line,
                    report.diagnostics
                );
            }
            for error in report.errors() {
                assert!(
                    case.expect
                        .iter()
                        .any(|expected| expected.code == error.code),
                    "{} as {format:?} has an unexpected error: {error:?}",
                    case.path
                );
            }
        }
        if case.canonical {
            let package = results[0]
                .1
                .package
                .as_ref()
                .expect("canonical case is valid");
            assert_eq!(
                package.to_yaml(),
                root_text(&case.path),
                "{} must re-serialize byte-identically",
                case.path
            );
        }
    }
}

#[test]
fn reference_single_file_needs_every_path_reference_inline() {
    let text = root_text("valid/reference");
    let checked = check_yaml(&text, &CheckOptions::domios());
    assert!(!checked.report.valid);
    let missing: Vec<&str> = checked
        .report
        .errors()
        .map(|error| {
            assert_eq!(error.code, DiagnosticCode::MissingFile, "{error:?}");
            error.path.as_str()
        })
        .collect();
    for path in [
        "files[3].path",
        "files[4].path",
        "skills[1].path",
        "automation[0].path",
        "workers.checker.instructions_file",
    ] {
        assert!(missing.contains(&path), "{path} missing from {missing:?}");
    }
}

#[test]
fn folder_and_archive_forms_export_the_same_bytes_deterministically() {
    let options = CheckOptions::domios();
    let folder = check_folder(&corpus().join("valid/reference"), &options)
        .package
        .expect("reference folder is valid");
    let archive = folder.to_archive().expect("archive writes");
    assert_eq!(archive, folder.to_archive().expect("archive writes again"));
    let reread = check_archive(&archive, &options)
        .package
        .expect("archive is valid");
    assert_eq!(reread.archive_entries(), folder.archive_entries());
    assert_eq!(reread.to_archive().expect("re-export"), archive);
    let entries = folder.archive_entries();
    assert_eq!(entries[0].0, "team.yaml");
    assert_eq!(entries.len(), folder.entries.len() + 1);
    let paths: Vec<&str> = entries[1..].iter().map(|(path, _)| path.as_str()).collect();
    let mut sorted = paths.clone();
    sorted.sort_unstable();
    assert_eq!(paths, sorted, "entries after team.yaml are in path order");
}

fn probe(instructions: &str) -> Package {
    let text = "name: dev.example/probe\nversion: 1.0.0\ndescription: Probe package.\nmanagers:\n  orchestrator:\n    instructions: |\n      Lead.\n";
    let mut package = check_yaml(text, &CheckOptions::default())
        .package
        .expect("probe base is valid");
    package.document.managers.orchestrator.instructions = Some(instructions.to_string());
    package
}

#[test]
fn odd_characters_and_huge_prompts_round_trip_exactly() {
    let huge: String = (0..20_000)
        .map(|index| format!("line {index} with {{braces}}, `ticks`, # hash and --- dashes\n"))
        .collect();
    let samples = [
        "```rust\nfn main() { println!(\"{}\", x); }\n```\n".to_string(),
        "  leading spaces\nthen text\n".to_string(),
        "\ttab-led line\n\tand another\n".to_string(),
        "# heading\n---\n...\nkey: value # not a comment\n- dash\n".to_string(),
        "unicode: привет 🌍 — ✓ 漢字\n".to_string(),
        "trailing spaces   \n   \nblank-ish line above\n".to_string(),
        "kept newlines\n\n\n".to_string(),
        "no trailing newline".to_string(),
        "\nleading empty line\n".to_string(),
        "windows\r\nline".to_string(),
        "nul \u{0} and bell \u{7}".to_string(),
        "1.20".to_string(),
        "true".to_string(),
        "~".to_string(),
        "yes".to_string(),
        "".to_string(),
        "*star".to_string(),
        "colon: space".to_string(),
        huge,
    ];
    let options = CheckOptions::default();
    for sample in samples {
        let package = probe(&sample);
        let yaml = package.to_yaml();
        let checked = check_yaml(&yaml, &options);
        let reread = checked.package.unwrap_or_else(|| {
            panic!(
                "{:?} failed to re-read: {:#?}\n{yaml}",
                sample.get(..60),
                checked.report.diagnostics
            )
        });
        assert_eq!(reread.document, package.document, "{:?}", sample.get(..60));
        assert_eq!(
            reread.to_yaml(),
            yaml,
            "writer is not idempotent for {:?}",
            sample.get(..60)
        );
    }
}

#[test]
fn crlf_input_is_normalized_and_canonical_output_is_lf() {
    let text = root_text("valid/reference-inline.team.yaml");
    let crlf = text.replace('\n', "\r\n");
    let checked = check_yaml(&crlf, &CheckOptions::domios());
    let package = checked.package.expect("CRLF input is valid");
    assert_eq!(package.to_yaml(), text);
}

fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let owned: Vec<(String, Vec<u8>)> = entries
        .iter()
        .map(|(name, data)| (name.to_string(), data.to_vec()))
        .collect();
    write_zip(&owned).expect("zip writes")
}

const MINIMAL: &[u8] = b"name: dev.example/probe\nversion: 1.0.0\ndescription: Probe package.\nmanagers:\n  orchestrator:\n    instructions: |\n      Lead.\n";

fn archive_codes(bytes: &[u8]) -> Vec<DiagnosticCode> {
    check_archive(bytes, &CheckOptions::domios())
        .report
        .errors()
        .map(|error| error.code)
        .collect()
}

#[test]
fn archive_entries_cannot_escape_or_hide_or_link() {
    assert!(
        archive_codes(&zip_with(&[("team.yaml", MINIMAL), ("../evil.txt", b"x")]))
            .contains(&DiagnosticCode::PathEscape)
    );
    assert!(
        archive_codes(&zip_with(&[("team.yaml", MINIMAL), ("/etc/evil", b"x")]))
            .contains(&DiagnosticCode::AbsolutePath)
    );
    assert!(archive_codes(&zip_with(&[
        ("team.yaml", MINIMAL),
        ("notes/extra.md", b"x")
    ]))
    .contains(&DiagnosticCode::UnreferencedEntry));
    assert_eq!(
        archive_codes(&zip_with(&[("other.yaml", MINIMAL)])),
        vec![DiagnosticCode::MissingRoot]
    );
    assert_eq!(
        archive_codes(b"not a zip"),
        vec![DiagnosticCode::ArchiveError]
    );

    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    writer.start_file("team.yaml", options).expect("start");
    std::io::Write::write_all(&mut writer, MINIMAL).expect("write");
    writer
        .add_symlink("linked.md", "../outside.md", options)
        .expect("symlink");
    let linked = writer.finish().expect("finish").into_inner();
    assert!(archive_codes(&linked).contains(&DiagnosticCode::LinkedEntry));
}

#[cfg(unix)]
#[test]
fn folder_links_are_refused_and_never_followed() {
    let dir = std::env::temp_dir().join(format!("atp-link-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::write(dir.join("team.yaml"), MINIMAL).expect("team.yaml");
    std::os::unix::fs::symlink("/etc/hosts", dir.join("hosts.md")).expect("symlink");
    let codes: Vec<DiagnosticCode> = check_folder(&dir, &CheckOptions::domios())
        .report
        .errors()
        .map(|error| error.code)
        .collect();
    std::fs::remove_dir_all(&dir).expect("cleanup");
    assert!(codes.contains(&DiagnosticCode::LinkedEntry), "{codes:?}");
}

#[test]
fn bounds_refuse_oversize_input_before_reading_it() {
    let tight = CheckOptions {
        limits: Limits {
            max_string_bytes: 64,
            max_entries: 2,
            max_yaml_bytes: 4096,
            ..Limits::default()
        },
        host: None,
    };
    let long = probe(&"x".repeat(100)).to_yaml();
    let codes = |text: &str| -> Vec<DiagnosticCode> {
        check_yaml(text, &tight)
            .report
            .errors()
            .map(|error| error.code)
            .collect()
    };
    assert!(codes(&long).contains(&DiagnosticCode::StringTooLong));
    assert_eq!(codes(&"#".repeat(5000)), vec![DiagnosticCode::TooLarge]);
    let crowded = zip_with(&[("team.yaml", MINIMAL), ("a.md", b"a"), ("b.md", b"b")]);
    let report = check_archive(&crowded, &tight).report;
    assert!(report
        .errors()
        .any(|error| error.code == DiagnosticCode::TooManyEntries));
}

#[test]
fn workers_without_instructions_follow_the_host_profile() {
    let text = "name: dev.example/probe\nversion: 1.0.0\ndescription: Probe package.\nmanagers:\n  orchestrator:\n    instructions: |\n      Lead.\nworkers:\n  qa:\n    description: Host QA.\n  scout:\n    description: Unknown here.\n";
    let format_only = check_yaml(text, &CheckOptions::default()).report;
    assert!(format_only.valid);
    assert_eq!(
        format_only
            .warnings()
            .filter(|warning| warning.code == DiagnosticCode::InheritsHostRole)
            .count(),
        2
    );
    let domios = check_yaml(text, &CheckOptions::domios()).report;
    let errors: Vec<&str> = domios.errors().map(|error| error.path.as_str()).collect();
    assert_eq!(errors, vec!["workers.scout.instructions"]);
}

#[test]
fn path_skills_are_named_by_their_frontmatter() {
    assert_eq!(
        frontmatter_name("---\nname: market-sizing\n---\nbody"),
        Some("market-sizing".into())
    );
    assert_eq!(
        frontmatter_name("---\ndescription: x\n---\nname: late"),
        None
    );
    assert_eq!(frontmatter_name("name: no-frontmatter"), None);
    let options = CheckOptions::domios();
    let package = check_folder(&corpus().join("valid/reference"), &options)
        .package
        .expect("reference is valid");
    let names: Vec<Option<String>> = package
        .document
        .skills
        .iter()
        .map(|skill| package.skill_name(skill))
        .collect();
    assert_eq!(
        names,
        vec![Some("source-check".into()), Some("market-sizing".into())]
    );
}

#[test]
fn grammars_follow_the_published_rules() {
    assert!(is_package_name("dev.domios/default"));
    assert!(is_package_name("example/research-desk"));
    assert!(!is_package_name("Research Desk"));
    assert!(!is_package_name("dev.domios/Default"));
    assert!(is_semver("2.1.0") && is_semver("1.0.0-rc.1+build.5"));
    assert!(!is_semver("1.0") && !is_semver("01.0.0") && !is_semver("two"));
    assert!(is_model_id("codex:gpt-6.1-sol") && is_model_id("open-router:meta/llama-4_x"));
    assert!(!is_model_id("opus") && !is_model_id("Claude:opus") && !is_model_id("claude:"));
    assert!(
        is_role_name("release_manager") && !is_role_name("release-manager") && !is_role_name("2qa")
    );
    assert_eq!(schedule_window("45m..2h"), Some((2_700, 7_200)));
    assert_eq!(schedule_window("2h..45m"), None);
    assert_eq!(schedule_window("30s..1h"), None);
    assert_eq!(schedule_window("0m..1h"), None);
}

#[test]
fn schema_requires_the_four_protocol_fields() {
    let schema = schema();
    let required: Vec<&str> = schema["required"]
        .as_array()
        .expect("required list")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    for field in ["name", "version", "description", "managers"] {
        assert!(
            required.contains(&field),
            "{field} missing from {required:?}"
        );
    }
}

#[test]
fn builtin_packages_are_valid_folder_packages() {
    let options = CheckOptions::domios();
    for name in builtin::names() {
        let checked = builtin::check(&name, &options).expect("indexed builtin has entries");
        assert!(
            checked.report.valid,
            "{name}: {:#?}",
            checked.report.diagnostics
        );
    }
}

#[test]
fn authored_builtin_packages_are_indexed_and_valid() {
    let options = CheckOptions::domios();
    let names = builtin::names();
    for (folder, package) in [
        ("release-train", "dev.domios/release-train"),
        ("qa-sweep", "dev.domios/qa-sweep"),
        ("distributed-bets", "dev.domios/distributed-bets"),
    ] {
        assert!(
            names.iter().any(|name| name == folder),
            "{folder} missing from {names:?}"
        );
        let checked = builtin::check(folder, &options).expect("indexed builtin has entries");
        assert!(
            checked.report.valid,
            "{folder}: {:#?}",
            checked.report.diagnostics
        );
        let loaded = checked.package.expect("a valid package loads");
        assert_eq!(loaded.document.name, package);
        assert!(loaded.icon().is_some(), "{folder} has no usable icon");
    }
}
