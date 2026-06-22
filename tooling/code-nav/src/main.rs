//! code-nav — a dev-only, parse-on-demand structural navigator for the CodeTerm
//! repo. Built on tree-sitter (rust + typescript/tsx grammars).
//!
//! It keeps NO persistent index or cache: every invocation parses the files it
//! needs fresh, so results are always current and there is nothing to stale or
//! gitignore beyond `target/`. A full `src-tauri/src` scan parses in well under
//! a second, which is cheap enough that an on-demand model is the right default.
//!
//! Use it for STRUCTURAL questions (where is X defined, who implements trait Y,
//! what is this file's symbol outline). For flat "does this string occur" or
//! name-counting, ripgrep is faster — see AGENTS.md.
//!
//! Subcommands:
//!   outline <file> [--json]   symbol outline of one file (kind, name, line)
//!   def     <dir> <name>      where a symbol is DEFINED across the tree (no call sites)
//!   impls   <dir> <Trait>     every `impl Trait for ...` block + the methods it defines
//!   query   <file> <scm>      run an arbitrary tree-sitter query, print captures

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use tree_sitter::{Language, Node, Parser, Query, QueryCursor, StreamingIterator};

const SKIP_DIRS: &[&str] = &["target", "node_modules", ".git", "dist", "gen", "target-check"];

fn lang_for(path: &Path) -> Option<(Language, &'static str)> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => Some((tree_sitter_rust::LANGUAGE.into(), "rust")),
        Some("ts") => Some((tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(), "ts")),
        Some("tsx") => Some((tree_sitter_typescript::LANGUAGE_TSX.into(), "tsx")),
        _ => None,
    }
}

fn node_line(n: Node) -> usize {
    n.start_position().row + 1
}

fn parse(src: &[u8], lang: &Language) -> tree_sitter::Tree {
    let mut parser = Parser::new();
    parser.set_language(lang).expect("load grammar");
    parser.parse(src, None).expect("parse")
}

fn walk(dir: &Path, exts: &[&str], out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            let name = p.file_name().and_then(|s| s.to_str());
            if !name.is_some_and(|n| SKIP_DIRS.contains(&n)) {
                walk(&p, exts, out);
            }
        } else if p
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|x| exts.contains(&x))
        {
            out.push(p);
        }
    }
}

// Outline query per language. Every capture is "kind" (the symbol kind) plus a
// `@def` capture on the node whose start line we report. Module-level TS decls
// are anchored to `(program ...)` so local `const`s inside function bodies are
// NOT captured as symbols.
fn outline_query(lang: &str) -> &'static str {
    match lang {
        "rust" => {
            r#"
            (function_item name: (identifier) @fn) @def
            (struct_item name: (type_identifier) @struct) @def
            (enum_item name: (type_identifier) @enum) @def
            (trait_item name: (type_identifier) @trait) @def
            (impl_item type: (type_identifier) @impl) @def
            (mod_item name: (identifier) @mod) @def
            (const_item name: (identifier) @const) @def
            (type_item name: (type_identifier) @type) @def
            "#
        }
        _ => {
            r#"
            (program (function_declaration name: (identifier) @fn) @def)
            (program (class_declaration name: (type_identifier) @class) @def)
            (program (interface_declaration name: (type_identifier) @interface) @def)
            (method_definition name: (property_identifier) @method) @def
            (program (lexical_declaration (variable_declarator name: (identifier) @const)) @def)
            (program (export_statement (function_declaration name: (identifier) @fn)) @def)
            (program (export_statement (class_declaration name: (type_identifier) @class)) @def)
            (program (export_statement (lexical_declaration (variable_declarator name: (identifier) @const))) @def)
            (program (type_alias_declaration name: (type_identifier) @type) @def)
            (program (export_statement (type_alias_declaration name: (type_identifier) @type)) @def)
            (program (enum_declaration name: (identifier) @enum) @def)
            "#
        }
    }
}

fn run_query(src: &[u8], lang: &Language, query_str: &str) -> Result<Vec<(String, String, usize)>, String> {
    let query = Query::new(lang, query_str).map_err(|e| format!("query error: {e:?}"))?;
    let tree = parse(src, lang);
    let names: Vec<&str> = query.capture_names().to_vec();
    let mut cursor = QueryCursor::new();
    let mut out = vec![];
    let mut it = cursor.matches(&query, tree.root_node(), src);
    while let Some(m) = it.next() {
        let mut def_line = 0usize;
        let mut kind = String::new();
        let mut name = String::new();
        for cap in m.captures {
            let cname = names[cap.index as usize];
            if cname == "def" {
                def_line = node_line(cap.node);
            } else {
                kind = cname.to_string();
                name = cap.node.utf8_text(src).unwrap_or("?").to_string();
            }
        }
        if !name.is_empty() {
            out.push((kind, name, def_line));
        }
    }
    out.sort_by_key(|(_, _, l)| *l);
    Ok(out)
}

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn cmd_outline(path: &Path, as_json: bool) -> Result<(), String> {
    let (lang, lname) = lang_for(path).ok_or_else(|| format!("unsupported file: {}", path.display()))?;
    let src = fs::read(path).map_err(|e| e.to_string())?;
    let syms = run_query(&src, &lang, outline_query(lname))?;
    if as_json {
        let items: Vec<String> = syms
            .iter()
            .map(|(k, n, l)| {
                format!(
                    "{{\"kind\":\"{}\",\"name\":\"{}\",\"line\":{}}}",
                    json_escape(k),
                    json_escape(n),
                    l
                )
            })
            .collect();
        println!(
            "{{\"file\":\"{}\",\"symbols\":[{}]}}",
            json_escape(&path.display().to_string()),
            items.join(",")
        );
    } else {
        println!("# {} ({} symbols)", path.display(), syms.len());
        for (kind, name, line) in syms {
            println!("{:>5}  {:<10} {}", line, kind, name);
        }
    }
    Ok(())
}

// Find where a symbol is DEFINED (outline match on the exact name), across
// .rs/.ts/.tsx — ignores call sites, comments and strings structurally.
fn cmd_def(dir: &Path, name: &str) -> Result<(), String> {
    let mut files = vec![];
    walk(dir, &["rs", "ts", "tsx"], &mut files);
    let mut hits = 0;
    for path in &files {
        let Some((lang, lname)) = lang_for(path) else { continue };
        let src = fs::read(path).map_err(|e| e.to_string())?;
        for (kind, sym, line) in run_query(&src, &lang, outline_query(lname))? {
            if sym == name {
                let rel = path.strip_prefix(dir).unwrap_or(path).display();
                println!("{}:{}  {} {}", rel, line, kind, sym);
                hits += 1;
            }
        }
    }
    if hits == 0 {
        println!("(no definition found for '{name}')");
    }
    Ok(())
}

// Every `impl Trait for Type` block (Rust only) + the method names it defines.
fn cmd_impls(dir: &Path, trait_name: &str) -> Result<(), String> {
    let lang: Language = tree_sitter_rust::LANGUAGE.into();
    let mut files = vec![];
    walk(dir, &["rs"], &mut files);
    println!("# impl {trait_name} for ...");
    let mut found = 0;
    for path in &files {
        let src = fs::read(path).map_err(|e| e.to_string())?;
        let tree = parse(&src, &lang);
        let mut stack = vec![tree.root_node()];
        while let Some(n) = stack.pop() {
            for i in 0..n.child_count() {
                stack.push(n.child(i).unwrap());
            }
            if n.kind() != "impl_item" {
                continue;
            }
            let tname = n
                .child_by_field_name("trait")
                .and_then(|t| t.utf8_text(&src).ok())
                .unwrap_or("");
            if tname != trait_name {
                continue;
            }
            found += 1;
            let for_type = n
                .child_by_field_name("type")
                .and_then(|t| t.utf8_text(&src).ok())
                .unwrap_or("?");
            let rel = path.strip_prefix(dir).unwrap_or(path).display();
            println!("{}:{}  impl {} for {}", rel, node_line(n), trait_name, for_type);
            if let Some(body) = n.child_by_field_name("body") {
                for i in 0..body.child_count() {
                    let c = body.child(i).unwrap();
                    if c.kind() == "function_item" {
                        if let Some(nm) = c.child_by_field_name("name") {
                            println!("        fn {}  (L{})", nm.utf8_text(&src).unwrap_or("?"), node_line(nm));
                        }
                    }
                }
            }
        }
    }
    if found == 0 {
        println!("(no impls found for '{trait_name}')");
    }
    Ok(())
}

fn cmd_query(path: &Path, scm: &str) -> Result<(), String> {
    let (lang, _) = lang_for(path).ok_or_else(|| format!("unsupported file: {}", path.display()))?;
    let src = fs::read(path).map_err(|e| e.to_string())?;
    for (kind, name, line) in run_query(&src, &lang, scm)? {
        println!("L{:<5} {:<14} {}", line, kind, name);
    }
    Ok(())
}

const USAGE: &str = "code-nav — dev-only structural navigator (parse-on-demand, no index)

USAGE:
  code-nav outline <file> [--json]   symbol outline of one .rs/.ts/.tsx file
  code-nav def     <dir> <name>      where a symbol is defined (no call sites)
  code-nav impls   <dir> <Trait>     every impl block for a trait + its methods
  code-nav query   <file> <scm>      run an arbitrary tree-sitter query

EXAMPLES:
  code-nav outline src-tauri/src/service/orchestrator.rs
  code-nav def     src-tauri/src spawn_agent
  code-nav impls   src-tauri/src AgentProvider";

fn main() {
    let args: Vec<String> = env::args().collect();
    let res = match args.get(1).map(String::as_str) {
        Some("outline") if args.len() >= 3 => {
            cmd_outline(Path::new(&args[2]), args.iter().any(|a| a == "--json"))
        }
        Some("def") if args.len() >= 4 => cmd_def(Path::new(&args[2]), &args[3]),
        Some("impls") if args.len() >= 4 => cmd_impls(Path::new(&args[2]), &args[3]),
        Some("query") if args.len() >= 4 => cmd_query(Path::new(&args[2]), &args[3]),
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };
    if let Err(e) = res {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
