//! Embedded CodeTerm docs. Bundled at compile time via include_str! so the
//! CLI ships with them — agents can `codeterm docs <name>` on demand without
//! polluting the system prompt.

pub(crate) const DOCS: &[(&str, &str)] = &[
    ("CONFIG", include_str!("../../../docs/CONFIG.md")),
    ("API", include_str!("../../../docs/API.md")),
    ("MESH", include_str!("../../../docs/MESH.md")),
    ("MEMORY", include_str!("../../../docs/MEMORY.md")),
    ("BUILD", include_str!("../../../docs/BUILD.md")),
    ("HTML", include_str!("../../../docs/HTML.md")),
    ("PROMPTS", include_str!("../../../docs/PROMPTS.md")),
    ("SKILLS", include_str!("../../../docs/SKILLS.md")),
    ("TESTING", include_str!("../../../docs/TESTING.md")),
    (
        "TELEGRAM-SETUP",
        include_str!("../../../docs/TELEGRAM-SETUP.md"),
    ),
    ("SECRETS", include_str!("../../../docs/SECRETS.md")),
    ("ROUTING", include_str!("../../../docs/ROUTING.md")),
    ("TEXT-HOOKS", include_str!("../../../docs/TEXT-HOOKS.md")),
    ("TRANSCRIBE", include_str!("../../../docs/TRANSCRIBE.md")),
    (
        "ORCHESTRATION",
        include_str!("../../../docs/ORCHESTRATION.md"),
    ),
    (
        "PLAN-EDITING",
        include_str!("../../../docs/PLAN-EDITING.md"),
    ),
    ("LANES", include_str!("../../../docs/LANES.md")),
    ("PLANNER", include_str!("../../../docs/PLANNER.md")),
    (
        "BOUNCE",
        include_str!("../../../src-tauri/prompts/bounce_unwinding_contract.md"),
    ),
    (
        "TEST-AUTHORING",
        include_str!("../../../docs/contributing/test-authoring.md"),
    ),
];

const CODETERM_TAB_PREVIEW_LINES: usize = 120;

pub fn handle(name: Option<String>, full: bool, head: Option<usize>) {
    match name {
        None => list_docs(),
        Some(n) => match render_doc(&n, full, head, is_codeterm_tab()) {
            Ok(rendered) => print!("{}", rendered),
            Err(available) => {
                eprintln!("No doc named '{}'. Available:", n);
                for name in available {
                    eprintln!("  {}", name);
                }
                std::process::exit(1);
            }
        },
    }
}

pub fn handle_doc(name: String) {
    let resolved = if name.eq_ignore_ascii_case("orchestration") {
        "ORCHESTRATION"
    } else {
        name.as_str()
    };
    match render_doc(resolved, true, None, false) {
        Ok(rendered) => print!("{}", rendered),
        Err(available) => {
            eprintln!("No doc named '{}'. Available:", name);
            for name in available {
                eprintln!("  {}", name);
            }
            std::process::exit(1);
        }
    }
}

fn list_docs() {
    println!("Available docs (use `codeterm docs <name>` to read):");
    for (name, body) in DOCS {
        let lines = body.lines().count();
        let preview = first_heading(body).unwrap_or("(no heading)");
        println!("  {:<10} {} lines  — {}", name, lines, preview);
    }
}

fn render_doc(
    name: &str,
    full: bool,
    head: Option<usize>,
    in_codeterm_tab: bool,
) -> Result<String, Vec<&'static str>> {
    let upper = name.to_ascii_uppercase();
    for (n, body) in DOCS {
        if *n == upper {
            let limit = if full {
                None
            } else {
                head.or_else(|| in_codeterm_tab.then_some(CODETERM_TAB_PREVIEW_LINES))
            };
            return Ok(render_body(n, body, limit));
        }
    }
    Err(DOCS.iter().map(|(n, _)| *n).collect())
}

fn render_body(name: &str, body: &str, limit: Option<usize>) -> String {
    let Some(limit) = limit else {
        return body.to_string();
    };
    let lines: Vec<&str> = body.lines().collect();
    if lines.len() <= limit {
        return body.to_string();
    }
    let mut out = lines
        .iter()
        .take(limit)
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    out.push_str(&format!(
        "\n\n[CodeTerm docs output truncated: showing {}/{} lines from {}. Use `codeterm docs {} --full` for the complete doc or `--head N` for a different preview.]\n",
        limit,
        lines.len(),
        name,
        name
    ));
    out
}

fn first_heading(body: &str) -> Option<&str> {
    body.lines()
        .find(|l| l.trim_start().starts_with("# "))
        .map(|l| l.trim_start().trim_start_matches('#').trim())
}

fn is_codeterm_tab() -> bool {
    std::env::var_os("CODETERM_TAB_ID").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codeterm_tab_preview_caps_secrets_doc() {
        let rendered = render_doc("SECRETS", false, None, true).expect("SECRETS doc exists");

        assert!(
            rendered.contains("CodeTerm docs output truncated"),
            "large docs must not stream in full by default inside agent Tabs"
        );
        assert!(
            rendered.lines().count()
                < DOCS
                    .iter()
                    .find(|(n, _)| *n == "SECRETS")
                    .unwrap()
                    .1
                    .lines()
                    .count(),
            "preview should be shorter than the full SECRETS playbook"
        );
        assert!(
            rendered.contains("codeterm docs SECRETS --full"),
            "preview must tell the caller how to request the full doc intentionally"
        );
    }

    #[test]
    fn full_preserves_complete_secrets_doc() {
        let full_body = DOCS.iter().find(|(n, _)| *n == "SECRETS").unwrap().1;
        let rendered = render_doc("SECRETS", true, None, true).expect("SECRETS doc exists");

        assert_eq!(rendered, full_body);
    }

    #[test]
    fn non_codeterm_stdout_preserves_complete_doc_by_default() {
        let full_body = DOCS.iter().find(|(n, _)| *n == "SECRETS").unwrap().1;
        let rendered = render_doc("SECRETS", false, None, false).expect("SECRETS doc exists");

        assert_eq!(rendered, full_body);
    }

    #[test]
    fn head_caps_output_everywhere() {
        let rendered = render_doc("API", false, Some(12), false).expect("API doc exists");

        assert!(rendered.contains("showing 12/"));
        assert!(rendered.lines().count() < 30);
    }

    #[test]
    fn orchestration_doc_alias_is_non_empty() {
        let rendered = render_doc("orchestration", true, None, false).expect("doc exists");

        assert!(rendered.contains("# Orchestration"));
        assert!(rendered.len() > 100);
    }

    #[test]
    fn lanes_doc_is_registered() {
        let rendered = render_doc("lanes", true, None, false).expect("LANES doc exists");

        assert!(rendered.contains("# Lane Lifecycle Standard"));
    }

    #[test]
    fn html_doc_is_registered() {
        let rendered = render_doc("html", true, None, false).expect("HTML doc exists");

        assert!(rendered.contains("# Sandboxed HTML in chat"));
        assert!(rendered.contains("ct.file.pick(options)"));
    }

    #[test]
    fn bounce_doc_preserves_registered_source() {
        let source = include_str!("../../../src-tauri/prompts/bounce_unwinding_contract.md");
        let rendered = render_doc("bounce", true, None, false).expect("BOUNCE doc exists");

        assert_eq!(rendered, source);
    }

    #[test]
    fn test_authoring_doc_is_registered() {
        let rendered =
            render_doc("test-authoring", true, None, false).expect("TEST-AUTHORING doc exists");

        assert!(rendered.contains("# Authoring automated tests"));
        assert!(rendered.contains("codeterm docs TEST-AUTHORING"));
    }
}
