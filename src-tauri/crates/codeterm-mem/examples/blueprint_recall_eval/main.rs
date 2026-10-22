#[cfg(not(feature = "embeddings"))]
compile_error!(
    "blueprint_recall_eval measures the global embedder; build with the embeddings feature"
);

mod confirm;
mod evaluate;
mod fixtures;
mod generate;
mod grid;
mod guard;
mod library;
mod probe;
mod quota;
mod report;
mod rerank;

use std::path::PathBuf;

const USAGE: &str = "usage: blueprint_recall_eval --generate [--jobs N] | --grid | --freeze | --confirm --set NAME --queries FILE --manifest FILE | --posthoc --queries FILE --label NAME --out FILE | --report | --query TEXT [--k N] [--json]";

fn value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == flag)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn required(args: &[String], flag: &str) -> Result<String, String> {
    value(args, flag).ok_or_else(|| format!("{flag} is required\n{USAGE}"))
}

fn run(args: &[String]) -> Result<(), String> {
    let has = |flag: &str| args.iter().any(|arg| arg == flag);
    if has("--generate") {
        let jobs = value(args, "--jobs")
            .and_then(|jobs| jobs.parse().ok())
            .unwrap_or(6);
        generate::generate(jobs)
    } else if has("--grid") {
        grid::run_grid()
    } else if has("--freeze") {
        grid::freeze()
    } else if has("--confirm") {
        confirm::confirm(
            &required(args, "--set")?,
            &PathBuf::from(required(args, "--queries")?),
            &PathBuf::from(required(args, "--manifest")?),
        )
    } else if has("--posthoc") {
        confirm::posthoc(
            &PathBuf::from(required(args, "--queries")?),
            &required(args, "--label")?,
            &PathBuf::from(required(args, "--out")?),
        )
    } else if has("--probe") {
        probe::probe(&required(args, "--probe")?)
    } else if has("--report") {
        report::report()
    } else if has("--query") {
        confirm::query(
            &required(args, "--query")?,
            value(args, "--k").and_then(|k| k.parse().ok()).unwrap_or(5),
            has("--json"),
        )
    } else {
        Err(USAGE.into())
    }
}

fn main() {
    quota::stash_cli_env();
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("CODETERM_") {
            std::env::remove_var(key);
        }
    }
    if std::env::var_os("HF_HUB_OFFLINE").is_none() {
        std::env::set_var("HF_HUB_OFFLINE", "1");
    }
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if let Err(error) = run(&args) {
        eprintln!("{error}");
        std::process::exit(2);
    }
}
