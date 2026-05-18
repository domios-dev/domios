use std::env;
use std::fs;
use std::process::ExitCode;

use codeterm_lib::agent::registry::AgentRegistry;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let provider = args.next().unwrap_or_else(|| "claude".to_string());
    let path = match args.next() {
        Some(p) => p,
        None => {
            eprintln!("usage: parser-eval <provider> <fixture_path>");
            return ExitCode::from(2);
        }
    };
    let raw = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("read {}: {}", path, e);
            return ExitCode::from(2);
        }
    };
    let reg = AgentRegistry::new();
    reg.register_provider(Box::new(codeterm_lib::plugin::agent_claude::ClaudeProvider::new()));
    let result = reg.with_provider(&provider, |p| {
        p.usage_parser().map(|parser| parser.parse(&raw))
    });
    let parsed = match result {
        Some(Some(r)) => r,
        Some(None) => {
            eprintln!("provider {} has no usage_parser", provider);
            return ExitCode::from(2);
        }
        None => {
            eprintln!("unknown provider: {}", provider);
            return ExitCode::from(2);
        }
    };
    match parsed {
        Ok(snap) => {
            println!("provider:           {}", snap.provider);
            println!("session_pct:        {:?}", snap.session_pct);
            println!("session_resets_at:  {:?}", snap.session_resets_at_ms);
            println!("weekly_pct:         {:?}", snap.weekly_pct);
            println!("weekly_resets_at:   {:?}", snap.weekly_resets_at_ms);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("parse error: {}", e);
            ExitCode::from(1)
        }
    }
}
