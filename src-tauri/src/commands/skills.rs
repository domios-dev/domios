pub(super) fn extract_revision(content: &str) -> u32 {
    content
        .lines()
        .find_map(|l| {
            let trimmed = l.trim();
            if trimmed.starts_with("<!-- revision:") && trimmed.ends_with("-->") {
                let inner = trimmed
                    .trim_start_matches("<!-- revision:")
                    .trim_end_matches("-->")
                    .trim();
                inner.parse().ok()
            } else {
                None
            }
        })
        .unwrap_or(0)
}

fn find_bundled_skills_dir() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        let exe_dir = exe.parent().unwrap_or(std::path::Path::new("."));

        let adjacent = exe_dir.join("skills");
        if adjacent.exists() {
            return adjacent;
        }

        #[cfg(target_os = "macos")]
        {
            let resources = exe_dir.join("../Resources/skills");
            if resources.exists() {
                return resources.canonicalize().unwrap_or(resources);
            }
        }

        let dev = exe_dir.join("../../../skills");
        if dev.exists() {
            return dev.canonicalize().unwrap_or(dev);
        }

        let dev_deep = exe_dir.join("../../../../skills");
        if dev_deep.exists() {
            return dev_deep.canonicalize().unwrap_or(dev_deep);
        }
    }
    std::path::PathBuf::from("skills")
}

pub fn seed_bundled_skills() {
    let bundled = find_bundled_skills_dir();
    let canonical = crate::paths::data_dir().join("skills");
    let _ = std::fs::create_dir_all(&canonical);

    let Ok(entries) = std::fs::read_dir(&bundled) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let id = entry.file_name();
        let src_skill = entry.path().join("SKILL.md");
        let dst_dir = canonical.join(&id);
        let dst_skill = dst_dir.join("SKILL.md");

        if dst_skill.exists() {
            let src_rev = std::fs::read_to_string(&src_skill)
                .map(|c| extract_revision(&c))
                .unwrap_or(0);
            let dst_rev = std::fs::read_to_string(&dst_skill)
                .map(|c| extract_revision(&c))
                .unwrap_or(0);
            if dst_rev >= src_rev {
                continue;
            }
        }

        let _ = std::fs::create_dir_all(&dst_dir);
        if let Err(e) = copy_dir_recursive(&entry.path(), &dst_dir) {
            log::warn!("[skills] Failed to seed {:?}: {}", id, e);
            continue;
        }
        log::info!("[skills] Seeded {:?} to canonical dir", id);

        // The `codeterm` skill SKILL.md is a template with a {codeterm_cli} placeholder. Substitute the shared CLI reference (single source of truth,
        // also injected into the GA prompt) so project agents and the GA see the same command surface.
        if id.to_string_lossy() == "codeterm" {
            if let Ok(body) = std::fs::read_to_string(&dst_skill) {
                if body.contains("{codeterm_cli}") {
                    let rendered = body.replace("{codeterm_cli}", crate::prompts::CODETERM_CLI);
                    if let Err(e) = std::fs::write(&dst_skill, rendered) {
                        log::warn!(
                            "[skills] Failed to render codeterm SKILL.md placeholder: {}",
                            e
                        );
                    }
                }
            }
        }
    }
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())?.flatten() {
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            std::fs::create_dir_all(&dst_path).map_err(|e| e.to_string())?;
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Mirror canonical `~/.codeterm/skills/` into provider dirs agents load from (#178).
pub fn mirror_skills_to_providers() {
    let canonical = crate::paths::data_dir().join("skills");
    let Some(home) = dirs::home_dir() else {
        return;
    };
    let targets = [
        home.join(".claude").join("skills"),
        home.join(".agents").join("skills"),
    ];

    let Ok(entries) = std::fs::read_dir(&canonical) else {
        return;
    };
    let skill_dirs: Vec<_> = entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();

    for target in &targets {
        for entry in &skill_dirs {
            let id = entry.file_name();
            let dst_dir = target.join(&id);
            if !should_mirror_skill(&entry.path().join("SKILL.md"), &dst_dir.join("SKILL.md")) {
                continue;
            }
            let _ = std::fs::create_dir_all(&dst_dir);
            match copy_dir_recursive(&entry.path(), &dst_dir) {
                Ok(()) => log::info!("[skills] mirrored {:?} -> {}", id, dst_dir.display()),
                Err(e) => log::warn!(
                    "[skills] mirror {:?} -> {} failed: {}",
                    id,
                    dst_dir.display(),
                    e
                ),
            }
        }
    }
}

/// Copy if provider copy is missing/older, or differs at the same revision. Never downgrades.
fn should_mirror_skill(src_skill: &std::path::Path, dst_skill: &std::path::Path) -> bool {
    let Ok(src) = std::fs::read_to_string(src_skill) else {
        return false;
    };
    let Ok(dst) = std::fs::read_to_string(dst_skill) else {
        return true;
    };
    let src_rev = extract_revision(&src);
    let dst_rev = extract_revision(&dst);
    if src_rev != dst_rev {
        return src_rev > dst_rev;
    }
    src != dst
}

#[cfg(test)]
mod mirror_tests {
    use super::*;

    fn write_skill(dir: &std::path::Path, body: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), body).unwrap();
    }

    #[test]
    fn mirror_decision_respects_revision_and_content() {
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("src");
        let dst_dir = tmp.path().join("dst");
        write_skill(&src_dir, "a\n<!-- revision: 5 -->");
        let src = src_dir.join("SKILL.md");
        let dst = dst_dir.join("SKILL.md");

        assert!(
            should_mirror_skill(&src, &dst),
            "missing provider copy -> mirror"
        );

        write_skill(&dst_dir, "a\n<!-- revision: 5 -->");
        assert!(!should_mirror_skill(&src, &dst), "identical -> skip");

        write_skill(&dst_dir, "OLD\n<!-- revision: 5 -->");
        assert!(
            should_mirror_skill(&src, &dst),
            "same rev, changed content -> mirror"
        );

        write_skill(&dst_dir, "x\n<!-- revision: 9 -->");
        assert!(
            !should_mirror_skill(&src, &dst),
            "provider newer -> never downgrade"
        );
    }
}
