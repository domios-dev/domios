pub use codeterm_events::*;

#[cfg(test)]
mod tests {
    fn read_sources(dir: &std::path::Path, exts: &[&str]) -> Vec<(std::path::PathBuf, String)> {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return out;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            if path.is_dir() {
                if name == "node_modules" || name == "target" || name == "dist" {
                    continue;
                }
                out.extend(read_sources(&path, exts));
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| exts.contains(&e))
            {
                if let Ok(txt) = std::fs::read_to_string(&path) {
                    out.push((path, txt));
                }
            }
        }
        out
    }

    fn event_family(name: &str) -> String {
        name.split(['$', '{']).next().unwrap_or(name).to_string()
    }

    #[test]
    fn event_seam_covers_all_frontend_listeners_dynamic_families() {
        assert_eq!(
            event_family("tab-status-${tabId}"),
            event_family("tab-status-{pty_id}")
        );
        assert_eq!(
            event_family("tab-chat-msg-${tabId}"),
            event_family("tab-chat-msg-{}")
        );
        assert_eq!(
            event_family("pty-resize-owner-changed"),
            "pty-resize-owner-changed"
        );
    }

    /// CI guard: no frontend-listened event may be delivered ONLY via a raw
    /// `app.emit()` — it must go through `emit_everywhere` (native + SSE bus) or
    /// carry a typed `SystemEvent` whose `type_name()` reaches the bus via
    /// `publish()`. A new raw emit of a listened event fails this test.
    #[test]
    fn event_seam_covers_all_frontend_listeners() {
        use std::collections::BTreeSet;
        use std::path::Path;
        let manifest = crate::test_support::fixture_manifest_dir();
        let frontend = Path::new(manifest).join("../src");
        let backend = Path::new(manifest).join("src");

        let packages = Path::new(manifest).join("../packages");
        let mut frontend_sources = read_sources(&frontend, &["ts", "tsx", "js", "jsx"]);
        frontend_sources.extend(read_sources(&packages, &["ts", "tsx", "js", "jsx"]));
        frontend_sources.retain(|(path, _)| !path.to_string_lossy().contains(".test."));
        let backend_sources = read_sources(&backend, &["rs"]);
        let const_re =
            regex::Regex::new(r#"const\s+(\w+)\s*(?::[^=;]+)?=\s*["']([^"']+)["']"#).unwrap();
        let mut constants = std::collections::BTreeMap::<String, BTreeSet<String>>::new();
        for (_, source) in frontend_sources.iter().chain(backend_sources.iter()) {
            for capture in const_re.captures_iter(source) {
                constants
                    .entry(capture[1].to_string())
                    .or_default()
                    .insert(capture[2].to_string());
            }
        }
        let listen_re = regex::Regex::new(
            r#"listen\w*\s*(?:<[^>]*>)?\s*\(\s*(?:["'`]([^"'`]+)["'`]|([A-Z][A-Z0-9_]+))"#,
        )
        .unwrap();
        let mut listened = BTreeSet::new();
        for (path, source) in &frontend_sources {
            for capture in listen_re.captures_iter(source) {
                if let Some(name) = capture.get(1) {
                    listened.insert(event_family(name.as_str()));
                } else if let Some(name) = capture.get(2) {
                    let values = constants.get(name.as_str()).unwrap_or_else(|| {
                        panic!(
                            "Unresolved listener constant {} in {}",
                            name.as_str(),
                            path.display()
                        )
                    });
                    listened.extend(values.iter().map(|value| event_family(value)));
                }
            }
        }
        assert!(
            listened.len() > 10,
            "frontend listen() scan found only {} names — path/regex broke",
            listened.len()
        );
        // invisible to the literal scan — enforce them explicitly.
        for name in [
            "graph-worker-spawned",
            "graph-worker-progress",
            "graph-worker-reported",
            "graph-worker-done",
        ] {
            listened.insert(name.to_string());
        }

        // Typed bus names = string literals returned by `SystemEvent::type_name`.
        let events_src =
            std::fs::read_to_string(Path::new(manifest).join("src/api/events.rs")).unwrap();
        let tn_start = events_src.find("fn type_name").expect("type_name fn");
        let tn_len = events_src[tn_start..]
            .find("\n    }")
            .expect("type_name end");
        let tn_body = &events_src[tn_start..tn_start + tn_len];
        let arm_re = regex::Regex::new(r#"=>\s*"([a-z0-9-]+)""#).unwrap();
        let typed: BTreeSet<String> = arm_re
            .captures_iter(tn_body)
            .map(|c| c[1].to_string())
            .collect();
        assert!(
            typed.contains("mux-state-changed"),
            "type_name scan broke: {typed:?}"
        );

        let raw_re = regex::Regex::new(
            r#"\.emit\s*\(\s*(?:format!\s*\(\s*)?(?:"([^"]+)"|([A-Z][A-Z0-9_]+))"#,
        )
        .unwrap();
        let mut offenders = Vec::new();
        for (path, source) in &backend_sources {
            for capture in raw_re.captures_iter(source) {
                let names = if let Some(value) = capture.get(1) {
                    BTreeSet::from([event_family(value.as_str())])
                } else if let Some(name) = capture.get(2) {
                    constants
                        .get(name.as_str())
                        .unwrap_or_else(|| {
                            panic!(
                                "Unresolved emitted constant {} in {}",
                                name.as_str(),
                                path.display()
                            )
                        })
                        .iter()
                        .map(|value| event_family(value))
                        .collect()
                } else {
                    continue;
                };
                for name in names {
                    let native = path.ends_with("native_resize.rs")
                        && matches!(
                            name.as_str(),
                            "terminal-window-resize-start" | "terminal-window-resize-commit"
                        );
                    if listened.contains(&name) && !typed.contains(&name) && !native {
                        offenders.push(format!("{}: raw emit({name})", path.display()));
                    }
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "Frontend-listened events raw-emitted, bypassing the publish seam \
             (route through emit_everywhere or add a typed SystemEvent):\n{}",
            offenders.join("\n")
        );
    }
}
