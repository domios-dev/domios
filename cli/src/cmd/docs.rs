//! Embedded CodeTerm documentation.

use std::fmt::Write;

macro_rules! doc {
    ($name:literal, $purpose:literal, $path:literal) => {
        EmbeddedDoc {
            name: $name,
            purpose: $purpose,
            source: $path,
            body: include_str!($path),
        }
    };
}

pub(crate) struct EmbeddedDoc {
    pub name: &'static str,
    pub purpose: &'static str,
    /// Include path relative to this file — the only link back to the repository
    /// file, so coverage checks key on provenance rather than guessing from `name`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub source: &'static str,
    pub body: &'static str,
}

pub(crate) const DOCS: &[EmbeddedDoc] = &[
    doc!(
        "CONFIG",
        "Application configuration reference",
        "../../../prod-docs/CONFIG.md"
    ),
    doc!(
        "NOTIFICATIONS",
        "Notification levels, per-level surfaces, sound, and push channels",
        "../../../prod-docs/NOTIFICATIONS.md"
    ),
    doc!(
        "INTEGRATIONS",
        "Messaging, hooks, transcription, and secret-backed integration settings",
        "../../../prod-docs/INTEGRATIONS.md"
    ),
    doc!(
        "RUNTIME-CONFIG",
        "Runtime, UI, session, cron, and memory settings",
        "../../../prod-docs/RUNTIME-CONFIG.md"
    ),
    doc!(
        "API",
        "REST, socket, and CLI integration reference",
        "../../../prod-docs/API.md"
    ),
    doc!(
        "API-TABS",
        "Tab and workspace REST contracts",
        "../../../prod-docs/API-TABS.md"
    ),
    doc!(
        "API-REPAIR",
        "Tab identity repair REST contract",
        "../../../prod-docs/API-REPAIR.md"
    ),
    doc!(
        "API-ADOPT",
        "Tab live-PTY adopt REST contract",
        "../../../prod-docs/API-ADOPT.md"
    ),
    doc!(
        "API-EVENTS",
        "Event stream and full-state snapshot load contracts",
        "../../../prod-docs/API-EVENTS.md"
    ),
    doc!(
        "API-PLAN",
        "Plan surface contracts: task structure, goals, lanes, executors",
        "../../../prod-docs/API-PLAN.md"
    ),
    doc!(
        "MESH-BRIDGE",
        "Moving a bridge between mesh peers",
        "../../../prod-docs/MESH-BRIDGE.md"
    ),
    doc!(
        "API-INTEGRATIONS",
        "Bridge, hook, notification, and cron REST contracts",
        "../../../prod-docs/API-INTEGRATIONS.md"
    ),
    doc!(
        "API-MEMORY",
        "Memory and graph REST contracts",
        "../../../prod-docs/API-MEMORY.md"
    ),
    doc!(
        "API-SERVICES",
        "Service, activity, cron, and maintenance REST contracts",
        "../../../prod-docs/API-SERVICES.md"
    ),
    doc!(
        "API-STATE",
        "Secrets, sessions, configuration, and update REST contracts",
        "../../../prod-docs/API-STATE.md"
    ),
    doc!(
        "API-AGENTS",
        "Agent restart, model switch, spawn, report, keep-awake, and batch REST contracts",
        "../../../prod-docs/API-AGENTS.md"
    ),
    doc!(
        "API-TEAMS",
        "Team, Team-bound plan, and task attempt REST contracts",
        "../../../prod-docs/API-TEAMS.md"
    ),
    doc!(
        "API-TRANSPORT",
        "Output streaming and socket JSON-RPC protocols",
        "../../../prod-docs/API-TRANSPORT.md"
    ),
    doc!(
        "API-CLI",
        "CLI, environment, and agent integration contracts",
        "../../../prod-docs/API-CLI.md"
    ),
    doc!(
        "TAB-LIFECYCLE",
        "Tab lifecycle journal (telemetry) and unconditional restore",
        "../../../prod-docs/TAB-LIFECYCLE.md"
    ),
    doc!(
        "MESH",
        "Peer discovery, trust, and remote routing",
        "../../../prod-docs/MESH.md"
    ),
    doc!(
        "MESH-SERVICES",
        "Mesh service registry, health, and peer management",
        "../../../prod-docs/MESH-SERVICES.md"
    ),
    doc!(
        "MEMORY",
        "Persistent memory, search, and graph behavior",
        "../../../prod-docs/MEMORY.md"
    ),
    doc!(
        "MEMORY-SERVICES",
        "Memory skills, workers, synchronization, and dreaming",
        "../../../prod-docs/MEMORY-SERVICES.md"
    ),
    doc!(
        "MEMORY-USAGE",
        "Memory agent behavior, CLI, REST, and UI",
        "../../../prod-docs/MEMORY-USAGE.md"
    ),
    doc!(
        "HTML",
        "Sandboxed chat HTML behavior",
        "../../../prod-docs/HTML.md"
    ),
    doc!(
        "PROMPTS",
        "Prompt sources and assembly",
        "../../../prod-docs/PROMPTS.md"
    ),
    doc!(
        "PROMPT-TIERS",
        "Prompt progressive-disclosure tiers",
        "../../../prod-docs/PROMPT-TIERS.md"
    ),
    doc!(
        "SKILLS",
        "CodeTerm skills and agent integration",
        "../../../prod-docs/SKILLS.md"
    ),
    doc!(
        "AGENT-CONTROL",
        "Agent hierarchy, command palette, and session integration",
        "../../../prod-docs/AGENT-CONTROL.md"
    ),
    doc!(
        "PROMPT-DETECTION",
        "Prompt detection architecture and verification",
        "../../../prod-docs/PROMPT-DETECTION.md"
    ),
    doc!(
        "TELEGRAM-SETUP",
        "Telegram bridge setup and troubleshooting",
        "../../../prod-docs/TELEGRAM-SETUP.md"
    ),
    doc!(
        "SECRETS",
        "Secret storage and synchronization",
        "../../../prod-docs/SECRETS.md"
    ),
    doc!(
        "ROUTING",
        "Message routing and reply channels",
        "../../../prod-docs/ROUTING.md"
    ),
    doc!(
        "TEXT-HOOKS",
        "Inbound text hook contracts",
        "../../../prod-docs/TEXT-HOOKS.md"
    ),
    doc!(
        "TRANSCRIBE",
        "Voice transcription integration",
        "../../../prod-docs/TRANSCRIBE.md"
    ),
    doc!(
        "ORCHESTRATION",
        "Team orchestration lifecycle",
        "../../../prod-docs/ORCHESTRATION.md"
    ),
    doc!(
        "ORCHESTRATION-TASKS",
        "Orchestration task lifecycle, milestones, and dispatch",
        "../../../prod-docs/ORCHESTRATION-TASKS.md"
    ),
    doc!(
        "ORCHESTRATION-OPERATIONS",
        "Orchestration verification, integration, heartbeat, and closeout",
        "../../../prod-docs/ORCHESTRATION-OPERATIONS.md"
    ),
    doc!(
        "PLAN-EDITING",
        "Bounded plan editing operations",
        "../../../prod-docs/PLAN-EDITING.md"
    ),
    doc!(
        "LANES",
        "Worker lanes, integration, and cleanup",
        "../../../prod-docs/LANES.md"
    ),
    doc!(
        "PLANNER",
        "Planner responsibilities and workflow",
        "../../../prod-docs/PLANNER.md"
    ),
    doc!(
        "BOUNCE",
        "Review bounce evidence and escalation",
        "../../../prod-docs/BOUNCE.md"
    ),
    doc!(
        "TEST-AUTHORING",
        "Focused test authoring rules",
        "../../../prod-docs/TEST-AUTHORING.md"
    ),
    doc!(
        "ESSENCE-MAP",
        "Source-backed essence map generation and validation",
        "../../../prod-docs/ESSENCE-MAP.md"
    ),
];

pub fn handle(name: Option<String>) {
    match name {
        None => print!("{}", render_list()),
        Some(name) => match render_doc(&name) {
            Ok(body) => print!("{body}"),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        },
    }
}

pub(crate) fn render_list() -> String {
    let mut output = String::from("Available docs:\n");
    for document in DOCS {
        writeln!(output, "  {:<16} {}", document.name, document.purpose).unwrap();
    }
    output.push_str("\nRead one complete document: codeterm docs <NAME>\n");
    output
}

pub(crate) fn render_help() -> String {
    format!(
        "Read embedded CodeTerm documentation.\n\nUsage: codeterm docs [NAME]\n\n{}",
        render_list()
    )
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct UnknownDocument {
    requested: String,
}

impl std::fmt::Display for UnknownDocument {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            formatter,
            "No doc named '{}'. Available docs:",
            self.requested
        )?;
        for document in DOCS {
            writeln!(formatter, "  {:<16} {}", document.name, document.purpose)?;
        }
        Ok(())
    }
}

pub(crate) fn render_doc(name: &str) -> Result<&'static str, UnknownDocument> {
    DOCS.iter()
        .find(|document| document.name.eq_ignore_ascii_case(name))
        .map(|document| document.body)
        .ok_or_else(|| UnknownDocument {
            requested: name.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::{render_doc, render_list, DOCS};

    #[test]
    fn docs_without_name_lists_available_documents_with_purposes() {
        let list = render_list();
        for document in DOCS {
            assert!(list.contains(document.name));
            assert!(list.contains(document.purpose));
            assert!(!document.purpose.trim().is_empty());
            assert!(!document.purpose.contains('\n'));
        }
    }

    #[test]
    fn named_doc_is_byte_exact_complete_document() {
        for document in DOCS {
            assert_eq!(render_doc(document.name), Ok(document.body));
        }
    }

    #[test]
    fn document_names_are_case_insensitive() {
        assert_eq!(render_doc("orchestration"), render_doc("ORCHESTRATION"));
    }

    #[test]
    fn unknown_document_lists_valid_names() {
        let error = render_doc("missing").unwrap_err().to_string();
        assert!(error.contains("No doc named 'missing'"));
        for document in DOCS {
            assert!(error.contains(document.name));
        }
    }

    #[test]
    fn registered_document_names_are_unique() {
        let mut names = HashSet::new();
        for document in DOCS {
            assert!(names.insert(document.name.to_ascii_lowercase()));
        }
    }

    #[test]
    fn every_doc_embedded_lives_in_prod_docs_and_nothing_else_does() {
        let embedded: HashSet<String> = DOCS
            .iter()
            .map(|document| {
                let file = document
                    .source
                    .strip_prefix("../../../prod-docs/")
                    .unwrap_or_else(|| {
                        panic!("{} is embedded from outside prod-docs", document.name)
                    });
                assert_eq!(
                    file,
                    format!("{}.md", document.name),
                    "{} must be embedded from prod-docs/{}.md",
                    document.name,
                    document.name
                );
                file.to_string()
            })
            .collect();

        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../prod-docs");
        let mut unregistered = Vec::new();
        for entry in std::fs::read_dir(dir).expect("read prod-docs/") {
            let path = entry.expect("prod-docs entry").path();
            assert!(
                path.is_file(),
                "prod-docs holds only embedded documents: {}",
                path.display()
            );
            let file = path.file_name().unwrap().to_string_lossy().into_owned();
            if !embedded.contains(&file) {
                unregistered.push(file);
            }
        }
        unregistered.sort();
        assert!(
            unregistered.is_empty(),
            "prod-docs files must be registered in DOCS, or moved to essences/: {unregistered:?}"
        );
    }

    #[test]
    fn repository_build_docs_are_not_user_facing() {
        assert!(render_doc("BUILD").is_err());
        assert!(render_doc("BUILD-REFERENCE").is_err());
    }

    #[test]
    fn registered_documents_have_one_title_and_fit_the_reading_boundary() {
        for document in DOCS {
            let mut fence = None;
            let mut titles = 0;
            for line in document.body.lines() {
                let trimmed = line.trim_start();
                let marker = trimmed.chars().next().filter(|ch| matches!(ch, '`' | '~'));
                let run = marker
                    .map(|ch| {
                        trimmed
                            .chars()
                            .take_while(|candidate| candidate == &ch)
                            .count()
                    })
                    .unwrap_or_default();
                if run >= 3 {
                    match fence {
                        None => fence = marker.map(|ch| (ch, run)),
                        Some((ch, minimum)) if marker == Some(ch) && run >= minimum => fence = None,
                        _ => {}
                    }
                    continue;
                }
                if fence.is_none() && line.starts_with("# ") {
                    titles += 1;
                }
            }
            assert!(
                document.body.starts_with("# "),
                "{} must start with one H1",
                document.name
            );
            assert_eq!(titles, 1, "{} must contain exactly one H1", document.name);
            assert!(
                document.body.lines().count() <= 240,
                "{} exceeds 240 lines",
                document.name
            );
            // A Windows checkout may carry CRLF; the reading budget counts the committed LF text.
            assert!(
                document.body.replace("\r\n", "\n").len() <= 24 * 1024,
                "{} exceeds 24 KiB",
                document.name
            );
        }
    }

    #[test]
    fn embedded_doc_references_name_registered_whole_documents() {
        let names = DOCS
            .iter()
            .map(|document| document.name)
            .collect::<HashSet<_>>();
        for document in DOCS {
            let mut seen = HashSet::new();
            for fragment in document.body.split('`') {
                let command = fragment.trim();
                let Some(target) = command.strip_prefix("codeterm docs ") else {
                    continue;
                };
                if target == "<NAME>" || target == "--help" {
                    continue;
                }
                assert!(
                    !target.contains(' '),
                    "{} uses a docs mode: {command}",
                    document.name
                );
                assert!(
                    names.contains(target),
                    "{} references unknown {target}",
                    document.name
                );
                assert_ne!(document.name, target, "{} links to itself", document.name);
                assert!(
                    seen.insert(target),
                    "{} repeats a whole-document reference to {target}",
                    document.name
                );
            }
        }
    }

    #[test]
    fn embedded_doc_reference_graph_is_acyclic() {
        let graph = DOCS
            .iter()
            .map(|document| {
                let targets = document
                    .body
                    .lines()
                    .filter(|line| {
                        let line = line.to_ascii_lowercase();
                        line.contains("must read")
                            || line.contains("read first")
                            || line.contains("required reading")
                            || line.contains("before continuing")
                    })
                    .flat_map(|line| line.split('`'))
                    .filter_map(|fragment| fragment.trim().strip_prefix("codeterm docs "))
                    .filter(|target| !target.starts_with('<') && *target != "--help")
                    .collect::<Vec<_>>();
                (document.name, targets)
            })
            .collect::<HashMap<_, _>>();

        fn visit<'a>(
            name: &'a str,
            graph: &HashMap<&'a str, Vec<&'a str>>,
            active: &mut HashSet<&'a str>,
            complete: &mut HashSet<&'a str>,
        ) {
            if complete.contains(name) {
                return;
            }
            assert!(
                active.insert(name),
                "embedded docs reference cycle at {name}"
            );
            for target in graph.get(name).into_iter().flatten() {
                visit(target, graph, active, complete);
            }
            active.remove(name);
            complete.insert(name);
        }

        let mut active = HashSet::new();
        let mut complete = HashSet::new();
        for document in DOCS {
            visit(document.name, &graph, &mut active, &mut complete);
        }
    }
}
