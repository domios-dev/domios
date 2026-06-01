//! Embedded CodeTerm docs. Bundled at compile time via include_str! so the
//! CLI ships with them — agents can `codeterm docs <name>` on demand without
//! polluting the system prompt.

const DOCS: &[(&str, &str)] = &[
    ("CONFIG", include_str!("../../../docs/CONFIG.md")),
    ("API", include_str!("../../../docs/API.md")),
    ("MESH", include_str!("../../../docs/MESH.md")),
    ("MEMORY", include_str!("../../../docs/MEMORY.md")),
    ("BUILD", include_str!("../../../docs/BUILD.md")),
    ("PROMPTS", include_str!("../../../docs/PROMPTS.md")),
    ("SKILLS", include_str!("../../../docs/SKILLS.md")),
    ("TESTING", include_str!("../../../docs/TESTING.md")),
    ("TELEGRAM-SETUP", include_str!("../../../docs/TELEGRAM-SETUP.md")),
    ("SECRETS", include_str!("../../../docs/SECRETS.md")),
    ("TEXT-HOOKS", include_str!("../../../docs/TEXT-HOOKS.md")),
    ("TRANSCRIBE", include_str!("../../../docs/TRANSCRIBE.md")),
];

pub fn handle(name: Option<String>) {
    match name {
        None => list_docs(),
        Some(n) => read_doc(&n),
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

fn read_doc(name: &str) {
    let upper = name.to_ascii_uppercase();
    for (n, body) in DOCS {
        if *n == upper {
            print!("{}", body);
            return;
        }
    }
    eprintln!("No doc named '{}'. Available:", name);
    for (n, _) in DOCS {
        eprintln!("  {}", n);
    }
    std::process::exit(1);
}

fn first_heading(body: &str) -> Option<&str> {
    body.lines()
        .find(|l| l.trim_start().starts_with("# "))
        .map(|l| l.trim_start().trim_start_matches('#').trim())
}
