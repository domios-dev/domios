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

#[cfg(test)]
const DOCS_DIR_PREFIX: &str = "../../../docs/";

/// `docs/<file>.md` this entry embeds, or `None` for a source outside `docs/`
/// or in a nested directory.
#[cfg(test)]
pub(crate) fn docs_dir_file(source: &str) -> Option<&str> {
    source
        .strip_prefix(DOCS_DIR_PREFIX)
        .filter(|rest| !rest.contains('/'))
}

pub(crate) const DOCS: &[EmbeddedDoc] = &[
    doc!(
        "BEGIN_HERE",
        "Documentation contract: what stable docs hold and when to edit them",
        "../../../docs/BEGIN_HERE.md"
    ),
    doc!(
        "CAPABILITIES",
        "Capability map: what CodeTerm can do, entry points, invariants, status",
        "../../../docs/CAPABILITIES.md"
    ),
    doc!(
        "CONFIG",
        "Application configuration reference",
        "../../../docs/CONFIG.md"
    ),
    doc!(
        "NOTIFICATIONS",
        "Notification levels, per-level surfaces, sound, and push channels",
        "../../../docs/NOTIFICATIONS.md"
    ),
    doc!(
        "INTEGRATIONS",
        "Messaging, hooks, transcription, and secret-backed integration settings",
        "../../../docs/INTEGRATIONS.md"
    ),
    doc!(
        "RUNTIME-CONFIG",
        "Runtime, UI, session, cron, and memory settings",
        "../../../docs/RUNTIME-CONFIG.md"
    ),
    doc!(
        "API",
        "REST, socket, and CLI integration reference",
        "../../../docs/API.md"
    ),
    doc!(
        "API-TABS",
        "Tab and workspace REST contracts",
        "../../../docs/API-TABS.md"
    ),
    doc!(
        "API-REPAIR",
        "Tab identity repair REST contract",
        "../../../docs/API-REPAIR.md"
    ),
    doc!(
        "API-EVENTS",
        "Event stream and full-state snapshot load contracts",
        "../../../docs/API-EVENTS.md"
    ),
    doc!(
        "API-PLAN",
        "Plan surface contracts: task structure, goals, lanes, executors",
        "../../../docs/API-PLAN.md"
    ),
    doc!(
        "MESH-BRIDGE",
        "Moving a bridge between mesh peers",
        "../../../docs/MESH-BRIDGE.md"
    ),
    doc!(
        "API-INTEGRATIONS",
        "Bridge, hook, notification, and cron REST contracts",
        "../../../docs/API-INTEGRATIONS.md"
    ),
    doc!(
        "API-MEMORY",
        "Memory and graph REST contracts",
        "../../../docs/API-MEMORY.md"
    ),
    doc!(
        "API-SERVICES",
        "Service, activity, cron, and maintenance REST contracts",
        "../../../docs/API-SERVICES.md"
    ),
    doc!(
        "API-STATE",
        "Secrets, sessions, configuration, and update REST contracts",
        "../../../docs/API-STATE.md"
    ),
    doc!(
        "API-AGENTS",
        "Agent, plan, team, and update REST contracts",
        "../../../docs/API-AGENTS.md"
    ),
    doc!(
        "API-TRANSPORT",
        "Output streaming and socket JSON-RPC protocols",
        "../../../docs/API-TRANSPORT.md"
    ),
    doc!(
        "API-CLI",
        "CLI, environment, and agent integration contracts",
        "../../../docs/API-CLI.md"
    ),
    doc!(
        "TAB-LIFECYCLE",
        "Tab lifecycle journal (telemetry) and unconditional restore",
        "../../../docs/TAB-LIFECYCLE.md"
    ),
    doc!(
        "MESH",
        "Peer discovery, trust, and remote routing",
        "../../../docs/MESH.md"
    ),
    doc!(
        "MESH-SERVICES",
        "Mesh service registry, health, and peer management",
        "../../../docs/MESH-SERVICES.md"
    ),
    doc!(
        "MEMORY",
        "Persistent memory, search, and graph behavior",
        "../../../docs/MEMORY.md"
    ),
    doc!(
        "MEMORY-SERVICES",
        "Memory skills, workers, synchronization, and dreaming",
        "../../../docs/MEMORY-SERVICES.md"
    ),
    doc!(
        "MEMORY-USAGE",
        "Memory agent behavior, CLI, REST, and UI",
        "../../../docs/MEMORY-USAGE.md"
    ),
    doc!(
        "HTML",
        "Sandboxed chat HTML behavior",
        "../../../docs/HTML.md"
    ),
    doc!(
        "PROMPTS",
        "Prompt sources and assembly",
        "../../../docs/PROMPTS.md"
    ),
    doc!(
        "PROMPT-TIERS",
        "Prompt progressive-disclosure tiers",
        "../../../docs/PROMPT-TIERS.md"
    ),
    doc!(
        "SKILLS",
        "CodeTerm skills and agent integration",
        "../../../docs/SKILLS.md"
    ),
    doc!(
        "AGENT-CONTROL",
        "Agent hierarchy, command palette, and session integration",
        "../../../docs/AGENT-CONTROL.md"
    ),
    doc!(
        "TESTING",
        "Test layers and verification workflows",
        "../../../docs/TESTING.md"
    ),
    doc!(
        "E2E-TESTING",
        "E2E, cross-platform, and manual smoke workflows",
        "../../../docs/E2E-TESTING.md"
    ),
    doc!(
        "PROMPT-DETECTION",
        "Prompt detection architecture and verification",
        "../../../docs/PROMPT-DETECTION.md"
    ),
    doc!(
        "TELEGRAM-SETUP",
        "Telegram bridge setup and troubleshooting",
        "../../../docs/TELEGRAM-SETUP.md"
    ),
    doc!(
        "SECRETS",
        "Secret storage and synchronization",
        "../../../docs/SECRETS.md"
    ),
    doc!(
        "ROUTING",
        "Message routing and reply channels",
        "../../../docs/ROUTING.md"
    ),
    doc!(
        "TEXT-HOOKS",
        "Inbound text hook contracts",
        "../../../docs/TEXT-HOOKS.md"
    ),
    doc!(
        "TRANSCRIBE",
        "Voice transcription integration",
        "../../../docs/TRANSCRIBE.md"
    ),
    doc!(
        "ORCHESTRATION",
        "Team orchestration lifecycle",
        "../../../docs/ORCHESTRATION.md"
    ),
    doc!(
        "ORCHESTRATION-OPERATIONS",
        "Orchestration verification, integration, heartbeat, and closeout",
        "../../../docs/ORCHESTRATION-OPERATIONS.md"
    ),
    doc!(
        "PLAN-EDITING",
        "Bounded plan editing operations",
        "../../../docs/PLAN-EDITING.md"
    ),
    doc!(
        "LANES",
        "Worker lanes, integration, and cleanup",
        "../../../docs/LANES.md"
    ),
    doc!(
        "PLANNER",
        "Planner responsibilities and workflow",
        "../../../docs/PLANNER.md"
    ),
    doc!(
        "BOUNCE",
        "Review bounce evidence and escalation",
        "../../../src-tauri/prompts/bounce_unwinding_contract.md"
    ),
    doc!(
        "TEST-AUTHORING",
        "Focused test authoring rules",
        "../../../docs/contributing/test-authoring.md"
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
            assert!(
                document.body.len() <= 24 * 1024,
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
