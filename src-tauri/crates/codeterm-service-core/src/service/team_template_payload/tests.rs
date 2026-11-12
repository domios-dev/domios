use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;

use codeterm_team_package::{check_folder, CheckOptions, MemoryKind, PackageMemory, PackageSkill};

use super::*;
use crate::service::team_exchange::scrub::{scrub_package, ScrubContext};

#[derive(Default)]
struct FakePort {
    files: RefCell<BTreeMap<String, Vec<u8>>>,
    memory: RefCell<Vec<PackageMemory>>,
    skills: RefCell<Vec<SkillFolder>>,
    automation: RefCell<Vec<AutomationItem>>,
    history: u32,
    failing_skills: bool,
    orchestrator_live: bool,
}

impl FakePort {
    fn live() -> Self {
        Self {
            orchestrator_live: true,
            ..Self::default()
        }
    }
}

impl PayloadPort for FakePort {
    fn write_file(&self, _team_id: &str, path: &str, bytes: &[u8]) -> Result<(), String> {
        self.files
            .borrow_mut()
            .insert(path.to_string(), bytes.to_vec());
        Ok(())
    }
    fn files(&self, _team_id: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
        Ok(self.files.borrow().clone().into_iter().collect())
    }
    fn seed_memory(&self, _team_id: &str, memory: &PackageMemory) -> Result<(), String> {
        self.memory.borrow_mut().push(memory.clone());
        Ok(())
    }
    fn memory(&self, _team_id: &str) -> Result<TeamMemory, String> {
        Ok(TeamMemory {
            items: self.memory.borrow().clone(),
            history_excluded: self.history,
        })
    }
    fn register_skill(&self, _team_id: &str, skill: &SkillFolder) -> Result<Landed, String> {
        if self.failing_skills {
            return Err("opaque skill failure".into());
        }
        self.skills.borrow_mut().push(skill.clone());
        Ok(Landed::Now)
    }
    fn skills(&self, _team_id: &str) -> Result<Vec<SkillFolder>, String> {
        Ok(self.skills.borrow().clone())
    }
    fn apply_automation(&self, _team_id: &str, item: &AutomationItem) -> Result<Landed, String> {
        self.automation.borrow_mut().push(item.clone());
        Ok(if self.orchestrator_live {
            Landed::Now
        } else {
            Landed::Deferred
        })
    }
    fn automation(&self, _team_id: &str) -> Result<Vec<AutomationItem>, String> {
        Ok(self.automation.borrow().clone())
    }
}

fn reference() -> Package {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codeterm-team-package/fixtures/corpus/valid/reference");
    check_folder(&folder, &CheckOptions::domios())
        .package
        .expect("reference is valid")
}

#[test]
fn start_applies_only_opted_in_items_through_the_port() {
    let package = reference();
    let port = FakePort::live();
    let opt_out = vec![
        "file:templates/report.md".to_string(),
        "memory:1".to_string(),
        "skill:source-check".to_string(),
        "automation:stale-task-nudge".to_string(),
    ];
    let result = apply_with(&port, "team_1", &package, &opt_out);
    assert!(result.failed.is_empty() && result.deferred.is_empty());
    let mut skipped = result.skipped.clone();
    skipped.sort();
    let mut expected = opt_out.clone();
    expected.sort();
    assert_eq!(skipped, expected, "every opted-out item is skipped");
    assert!(result.applied.iter().all(|id| !opt_out.contains(id)));
    let all = payload_items(&package).len();
    assert_eq!(result.applied.len() + result.skipped.len(), all);

    let files = port.files.borrow();
    assert!(!files.contains_key("templates/report.md"));
    assert_eq!(
        files.get("data/competitors.csv").map(Vec::len),
        package.file_bytes("data/competitors.csv").map(<[u8]>::len)
    );
    assert!(
        files.contains_key("assets/logo.png"),
        "binary files land unchanged"
    );
    assert_eq!(
        port.memory.borrow().len(),
        package.document.memory.len() - 1
    );
    let skills = port.skills.borrow();
    assert!(skills.iter().all(|skill| skill.name != "source-check"));
    let sizing = skills
        .iter()
        .find(|skill| skill.name == "market-sizing")
        .expect("path skill");
    assert!(sizing.files.iter().any(|(path, _)| path == "SKILL.md"));
    let automation = port.automation.borrow();
    assert_eq!(automation.len(), 1);
    assert_eq!(automation[0].name, "research-digest");
}

#[test]
fn bake_collects_the_same_mapping_back_and_the_export_scrubs_it() {
    let package = reference();
    let port = FakePort {
        history: 3,
        ..FakePort::live()
    };
    apply_with(&port, "team_1", &package, &[]);
    port.memory.borrow_mut().push(PackageMemory {
        title: "Staging login".into(),
        kind: MemoryKind::Fact,
        body: "Use STAGING_PASSWORD=hunter2hunter2 on /home/ada/stage.".into(),
        tags: Vec::new(),
    });
    let descriptions: BTreeMap<String, String> = [(
        "data/competitors.csv".to_string(),
        "Known competitors".to_string(),
    )]
    .into_iter()
    .collect();
    let collected = collect_with(&port, "team_1", &descriptions).expect("collects");
    assert_eq!(collected.history_excluded, 3);
    assert_eq!(collected.memory.len(), package.document.memory.len() + 1);
    let csv = collected
        .files
        .iter()
        .find(|file| file.path == "data/competitors.csv")
        .unwrap();
    assert_eq!(csv.description.as_deref(), Some("Known competitors"));
    assert!(collected.entries.contains_key("assets/logo.png"));
    let names: Vec<&str> = collected
        .skills
        .iter()
        .filter_map(PackageSkill::inline_name)
        .collect();
    assert!(names.contains(&"market-sizing") && names.contains(&"source-check"));
    assert_eq!(
        collected.automation.len(),
        package.document.automation.len()
    );

    let mut baked = package.clone();
    baked.document.memory = collected.memory;
    let context = ScrubContext {
        personal_roots: vec!["/home/ada".into()],
        machines: Vec::new(),
    };
    let report = scrub_package(&mut baked, &context);
    let leaked = baked.document.memory.last().unwrap();
    assert_eq!(
        leaked.body,
        "Use STAGING_PASSWORD=[secret STAGING_PASSWORD] on {workspace}/stage."
    );
    assert!(report
        .secret_names
        .contains(&"STAGING_PASSWORD".to_string()));
}

#[test]
fn opting_out_removes_the_item_and_every_role_reference_to_it() {
    let package = reference();
    let trimmed = retain_opted_in(
        &package,
        &[
            "file:data/competitors.csv".to_string(),
            "skill:skills/market-sizing".to_string(),
        ],
    );
    assert!(trimmed
        .document
        .files
        .iter()
        .all(|file| file.path != "data/competitors.csv"));
    assert!(!trimmed
        .entries
        .keys()
        .any(|path| path.starts_with("skills/market-sizing/")));
    assert!(!trimmed.entries.contains_key("data/competitors.csv"));
    for (_, _, role) in trimmed.document.roles() {
        assert!(!role.files.contains(&"data/competitors.csv".to_string()));
        assert!(!role.skills.contains(&"market-sizing".to_string()));
    }
    let checked = codeterm_team_package::check_entries(
        codeterm_team_package::PackageFormat::Folder,
        trimmed.archive_entries(),
        Vec::new(),
        &CheckOptions::domios(),
    );
    assert!(checked.report.valid, "{:?}", checked.report.diagnostics);
    let icon = package.document.icon.clone().unwrap();
    let kept = retain_opted_in(&package, &[format!("file:{icon}")]);
    assert!(
        kept.document.files.iter().any(|file| file.path == icon),
        "the icon always travels"
    );
}

#[test]
fn knowledge_paths_round_trip_and_role_files_carry_descriptions() {
    let package = reference();
    let path = knowledge_path("team_1", "essences/how-we-research.md");
    assert!(path.ends_with("teams/team_1/knowledge/essences/how-we-research.md"));
    assert_eq!(
        package_path("team_1", &path).as_deref(),
        Some("essences/how-we-research.md")
    );
    assert_eq!(package_path("team_2", &path), None);
    let lead = package.document.role("orchestrator").unwrap();
    let refs = role_file_refs("team_1", &package, lead);
    assert_eq!(refs.len(), lead.files.len());
    assert_eq!(
        refs[0].description.as_deref(),
        Some("Our research method; read before planning")
    );
}

#[test]
fn service_port_writes_stay_inside_their_root_and_skip_links() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    for bad in [
        "../escape.md",
        "/abs.md",
        "a/../b.md",
        "C:/x.md",
        "a\\b.md",
        "",
    ] {
        assert!(
            service_port::write_under_for_test(&root, bad).is_err(),
            "{bad}"
        );
    }
    service_port::write_under_for_test(&root, "notes/a.md").unwrap();
    let tree = service_port::read_tree(&root).unwrap();
    assert_eq!(tree, vec![("notes/a.md".to_string(), b"x".to_vec())]);
}

#[test]
fn one_failed_item_never_stops_the_rest_and_blueprints_wait_for_the_orchestrator() {
    let package = reference();
    let port = FakePort {
        failing_skills: true,
        ..FakePort::default()
    };
    let result = apply_with(&port, "team_1", &package, &[]);
    let skill_ids: Vec<String> = payload_items(&package)
        .into_iter()
        .map(|item| item.id)
        .filter(|id| id.starts_with("skill:"))
        .collect();
    let failed: Vec<&str> = result.failed.iter().map(|f| f.id.as_str()).collect();
    assert_eq!(
        failed,
        skill_ids.iter().map(String::as_str).collect::<Vec<_>>()
    );
    assert!(result
        .failed
        .iter()
        .all(|failure| failure.reason.contains("opaque skill failure")));
    assert_eq!(
        result.deferred.len(),
        package.document.automation.len(),
        "every Blueprint after the failed skills still reached the port"
    );
    assert!(result
        .deferred
        .iter()
        .all(|id| id.starts_with("automation:")));
    assert_eq!(
        result.applied.len() + result.deferred.len() + result.failed.len(),
        payload_items(&package).len()
    );
}

#[test]
fn closing_a_team_removes_its_knowledge_and_held_skills_only() {
    let root = tempfile::tempdir().unwrap();
    let teams = root.path();
    for (team, part) in [
        ("team_gone", "knowledge"),
        ("team_gone", "skills/opaque-skill"),
        ("team_kept", "knowledge"),
    ] {
        let dir = teams.join(team).join(part);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("file.md"), b"x").unwrap();
    }
    remove_team_payload(teams, "team_gone").unwrap();
    assert!(!teams.join("team_gone/knowledge").exists());
    assert!(!teams.join("team_gone/skills").exists());
    assert!(teams.join("team_kept/knowledge/file.md").exists());
    remove_team_payload(teams, "team_gone").expect("an absent payload is already gone");
    assert!(remove_team_payload(teams, "../team_kept").is_err());
    assert!(teams.join("team_kept/knowledge/file.md").exists());
}

#[test]
fn an_emptied_team_folder_goes_and_a_registered_one_stays() {
    let root = tempfile::tempdir().unwrap();
    let teams = root.path();
    std::fs::create_dir_all(teams.join("team_empty/knowledge")).unwrap();
    std::fs::create_dir_all(teams.join("team_roles/knowledge")).unwrap();
    std::fs::write(teams.join("team_roles/opaque-registrations"), b"x").unwrap();
    remove_team_payload(teams, "team_empty").unwrap();
    remove_team_payload(teams, "team_roles").unwrap();
    assert!(!teams.join("team_empty").exists());
    assert!(teams.join("team_roles/opaque-registrations").exists());
}
