use super::{Handler, Invoke};
use regex::Regex;
use std::sync::OnceLock;

static HANDLER_RE: OnceLock<Regex> = OnceLock::new();
static INVOKE_RE: OnceLock<Regex> = OnceLock::new();

fn handler_re() -> &'static Regex {
    HANDLER_RE.get_or_init(|| {
        Regex::new(
            r"(?m)^\s*#\[\s*tauri::command(?:\s*\([^)]*\))?\s*\][\s\S]{0,200}?fn\s+([A-Za-z_][A-Za-z0-9_]*)",
        )
        .expect("compile handler regex")
    })
}

fn invoke_re() -> &'static Regex {
    INVOKE_RE.get_or_init(|| {
        Regex::new(r#"\binvoke\s*\(\s*['"`]([A-Za-z_][A-Za-z0-9_]*)['"`]"#)
            .expect("compile invoke regex")
    })
}

pub fn handlers_in_rust(source: &str) -> Vec<Handler> {
    let re = handler_re();
    let mut out = Vec::new();
    for m in re.captures_iter(source) {
        let name_match = m.get(1).expect("regex capture group 1");
        let name = name_match.as_str().to_string();
        let line = byte_to_line(source, name_match.start());
        out.push(Handler { name, line });
    }
    out
}

pub fn invokes_in_ts(source: &str) -> Vec<Invoke> {
    let re = invoke_re();
    let mut out = Vec::new();
    for m in re.captures_iter(source) {
        let cmd_match = m.get(1).expect("regex capture group 1");
        let command = cmd_match.as_str().to_string();
        let line = byte_to_line(source, cmd_match.start());
        out.push(Invoke { command, line });
    }
    out
}

fn byte_to_line(source: &str, byte_offset: usize) -> u32 {
    let mut line = 0u32;
    for (i, b) in source.bytes().enumerate() {
        if i >= byte_offset {
            return line;
        }
        if b == b'\n' {
            line += 1;
        }
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_basic_handler() {
        let src = "#[tauri::command]\nfn cmd_hello() {}\n";
        let h = handlers_in_rust(src);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].name, "cmd_hello");
    }

    #[test]
    fn finds_async_pub_handler() {
        let src = r#"
#[tauri::command]
pub async fn mux_create_workspace(state: State, name: String) -> Result<(), String> {
    Ok(())
}
"#;
        let h = handlers_in_rust(src);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].name, "mux_create_workspace");
    }

    #[test]
    fn finds_handler_with_attr_args_and_extra_attrs() {
        let src = r#"
#[tauri::command(rename_all = "snake_case")]
#[allow(dead_code)]
pub fn do_thing() {}
"#;
        let h = handlers_in_rust(src);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].name, "do_thing");
    }

    #[test]
    fn finds_invokes_double_and_single_quotes() {
        let src = r#"
import { invoke } from "@tauri-apps/api/core";
invoke("mux_create_workspace", { name });
invoke('mux_close_workspace', { id });
something_else_invoke("not_us");
"#;
        let i = invokes_in_ts(src);
        let names: Vec<&str> = i.iter().map(|v| v.command.as_str()).collect();
        assert!(names.contains(&"mux_create_workspace"), "got {names:?}");
        assert!(names.contains(&"mux_close_workspace"), "got {names:?}");
        assert!(!names.contains(&"not_us"), "word boundary should exclude suffix matches: {names:?}");
    }

    #[test]
    fn handler_line_zero_indexed() {
        let src = "fn unrelated() {}\n#[tauri::command]\nfn target() {}\n";
        let h = handlers_in_rust(src);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].line, 2);
    }

    #[test]
    fn ignores_commented_attributes() {
        let src = r#"
// #[tauri::command]
fn not_a_command() {}
"#;
        let h = handlers_in_rust(src);
        assert!(h.is_empty(), "got {h:?}");
    }
}
