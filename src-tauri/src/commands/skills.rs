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

/// Mirror the `codeterm` skill into provider dirs and prune everything else.
/// Provider dirs hold EXACTLY one skill — `codeterm`. All others are agents-only
/// (discovered via `codeterm mem search`, not provider-dir injection).
pub fn mirror_skills_to_providers() {
    let canonical = crate::paths::data_dir().join("skills");
    let Some(home) = dirs::home_dir() else {
        return;
    };
    let targets = [
        home.join(".claude").join("skills"),
        home.join(".agents").join("skills"),
    ];
    mirror_into_providers(&canonical, &targets);
}

/// Pure inner implementation — injectable paths make this testable without touching real home dirs.
fn mirror_into_providers(canonical: &std::path::Path, targets: &[std::path::PathBuf]) {
    // Install / update the `codeterm` skill in every provider dir.
    let codeterm_src = canonical.join("codeterm");
    for target in targets {
        let codeterm_dst = target.join("codeterm");
        if should_mirror_skill(
            &codeterm_src.join("SKILL.md"),
            &codeterm_dst.join("SKILL.md"),
        ) {
            let _ = std::fs::create_dir_all(&codeterm_dst);
            match copy_dir_recursive(&codeterm_src, &codeterm_dst) {
                Ok(()) => log::info!("[skills] mirrored codeterm -> {}", codeterm_dst.display()),
                Err(e) => log::warn!(
                    "[skills] mirror codeterm -> {} failed: {}",
                    codeterm_dst.display(),
                    e
                ),
            }
        }

        // Remove every non-codeterm entry — provider dirs must be exactly {codeterm}.
        let Ok(entries) = std::fs::read_dir(target) else {
            continue;
        };
        for entry in entries.flatten() {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            if entry.file_name() == "codeterm" {
                continue;
            }
            let path = entry.path();
            log::warn!(
                "[skills] pruning non-codeterm entry from provider dir: {}",
                path.display()
            );
            if let Err(e) = std::fs::remove_dir_all(&path) {
                log::warn!("[skills] failed to remove {}: {}", path.display(), e);
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

/// Build the task-brief block for a cron job's declared `--skill <slug>`.
/// Resolves the slug against the canonical skill dir (never the provider
/// dirs — those hold only `codeterm`, see `mirror_skills_to_providers`) and
/// gives the spawned agent the exact `SKILL.md` path to read directly,
/// instead of relying on the Skill tool or a filesystem search (issue #179:
/// neither resolves `~/.codeterm/skills/*` for a cron/spawned agent).
pub fn cron_skill_block(slug: &str) -> String {
    cron_skill_block_at(slug, &crate::paths::data_dir().join("skills"))
}

fn cron_skill_block_at(slug: &str, skills_root: &std::path::Path) -> String {
    let skill_md = skills_root.join(slug).join("SKILL.md");
    let path_str = skill_md.display().to_string();
    if skill_md.exists() {
        crate::prompts::CRON_SKILL_HINT
            .replace("{skill_slug}", slug)
            .replace("{skill_path}", &path_str)
    } else {
        crate::prompts::CRON_SKILL_MISSING
            .replace("{skill_slug}", slug)
            .replace("{skill_path}", &path_str)
    }
}

#[cfg(test)]
mod skill_block_tests {
    use super::cron_skill_block_at;

    #[test]
    fn found_skill_points_at_exact_path_and_forbids_skill_tool() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("scratch-drill-skill");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), "# Scratch drill\nBody.").unwrap();

        let block = cron_skill_block_at("scratch-drill-skill", tmp.path());
        assert!(block.contains("scratch-drill-skill"));
        assert!(block.contains("SKILL.md"));
        assert!(block.to_lowercase().contains("do not use the skill tool"));
        assert!(!block.to_lowercase().contains("find "));
    }

    #[test]
    fn missing_skill_tells_agent_to_report_not_search() {
        let tmp = tempfile::tempdir().unwrap();
        let block = cron_skill_block_at("nonexistent-slug", tmp.path());
        assert!(block.contains("nonexistent-slug"));
        assert!(block.to_lowercase().contains("cron complete"));
        assert!(block.to_lowercase().contains("find"));
        assert!(block.to_lowercase().contains("never"));
    }
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

    /// Restart-does-not-resurrect drill: calls the REAL mirror_into_providers against a
    /// tempdir fixture. Plants a stray in the provider dir, runs the mirror, asserts the
    /// provider dir ends up with exactly {codeterm}.
    #[test]
    fn mirror_installs_only_codeterm_and_prunes_strays() {
        let tmp = tempfile::tempdir().unwrap();
        let canonical = tmp.path().join("canonical");
        let provider = tmp.path().join("provider");

        // Canonical has codeterm + another skill (atlassian stays in canonical only).
        write_skill(&canonical.join("codeterm"), "codeterm skill body");
        write_skill(&canonical.join("atlassian"), "atlassian skill body");

        // Provider starts with a stray that should be pruned on mirror.
        write_skill(&provider.join("stray"), "orphan");

        // Call the real inner function with injected paths.
        mirror_into_providers(&canonical, &[provider.clone()]);

        // codeterm must be installed.
        assert!(
            provider.join("codeterm").join("SKILL.md").exists(),
            "codeterm SKILL.md must be present after mirror"
        );
        // stray must be gone — mirror actively prunes non-codeterm entries.
        assert!(
            !provider.join("stray").exists(),
            "stray was not pruned by mirror"
        );
        // atlassian must NOT have been copied — it stays in canonical only.
        assert!(
            !provider.join("atlassian").exists(),
            "non-codeterm canonical skill must not appear in provider dir"
        );
    }
}
