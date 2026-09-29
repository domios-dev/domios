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

const CODETERM_SKILL_ID: &str = "codeterm";
const PROVIDER_OWNERSHIP_FILE: &str = ".codeterm-owner";

/// Placeholder → the one semantic source that fills it. Shared fragments also
/// reach coordinating prompts, so the skill file carries no second copy.
fn codeterm_skill_render_map() -> [(&'static str, &'static str); 3] {
    [
        ("{codeterm_cli}", crate::prompts::CODETERM_CLI),
        (
            "{interactive_child_pty_operation}",
            crate::prompts::INTERACTIVE_CHILD_PTY_OPERATION.trim(),
        ),
        ("{agent_pad_rule}", crate::prompts::AGENT_PAD_RULE.trim()),
    ]
}

fn render_bundled_skill(id: &std::ffi::OsStr, content: &str) -> String {
    if id != std::ffi::OsStr::new(CODETERM_SKILL_ID) {
        return content.to_string();
    }
    codeterm_skill_render_map()
        .into_iter()
        .fold(content.to_string(), |body, (slot, source)| {
            body.replace(slot, source)
        })
}

pub fn seed_bundled_skills() {
    seed_bundled_skills_into(
        &find_bundled_skills_dir(),
        &crate::paths::data_dir().join("skills"),
    );
}

/// Pure inner implementation — injectable paths let tests exercise the real
/// render and gate without touching the machine's data dir.
fn seed_bundled_skills_into(bundled: &std::path::Path, canonical: &std::path::Path) {
    let _ = std::fs::create_dir_all(canonical);

    let Ok(entries) = std::fs::read_dir(bundled) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let id = entry.file_name();
        let src_dir = entry.path();
        let dst_dir = canonical.join(&id);

        if !should_copy_skill(&id, &src_dir, &dst_dir) {
            continue;
        }

        let _ = std::fs::create_dir_all(&dst_dir);
        if let Err(e) = copy_skill_rendered(&id, &src_dir, &dst_dir) {
            log::warn!("[skills] Failed to seed {:?}: {}", id, e);
            continue;
        }
        log::info!("[skills] Seeded {:?} to canonical dir", id);
    }
}

/// Copy when the destination is missing or differs from the rendered source anywhere
/// in the tree. Revision is a never-downgrade guard, never the copy decision.
fn should_copy_skill(
    id: &std::ffi::OsStr,
    src_dir: &std::path::Path,
    dst_dir: &std::path::Path,
) -> bool {
    if !src_dir.join("SKILL.md").exists() {
        return false;
    }
    if destination_is_newer(&src_dir.join("SKILL.md"), &dst_dir.join("SKILL.md")) {
        return false;
    }
    skill_tree_differs(id, src_dir, dst_dir)
}

fn destination_is_newer(src_skill: &std::path::Path, dst_skill: &std::path::Path) -> bool {
    let (Ok(src), Ok(dst)) = (
        std::fs::read_to_string(src_skill),
        std::fs::read_to_string(dst_skill),
    ) else {
        return false;
    };
    extract_revision(&dst) > extract_revision(&src)
}

/// Render-aware because the source is pre-render and the destination post-render;
/// directory-scoped because SKILL.md alone hides an edited `references/` file.
fn skill_tree_differs(
    id: &std::ffi::OsStr,
    src_dir: &std::path::Path,
    dst_dir: &std::path::Path,
) -> bool {
    let Ok(entries) = std::fs::read_dir(src_dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let src_path = entry.path();
        let dst_path = dst_dir.join(entry.file_name());
        if src_path.is_dir() {
            if skill_tree_differs(id, &src_path, &dst_path) {
                return true;
            }
            continue;
        }
        let Ok(src) = std::fs::read(&src_path) else {
            continue;
        };
        let Ok(dst) = std::fs::read(&dst_path) else {
            return true;
        };
        let differs = match (std::str::from_utf8(&src), std::str::from_utf8(&dst)) {
            (Ok(src_text), Ok(dst_text)) => render_bundled_skill(id, src_text) != dst_text,
            _ => src != dst,
        };
        if differs {
            return true;
        }
    }
    false
}

/// Copies through the same render the gate compares against, so a copied tree
/// compares equal on the next start instead of reseeding forever.
fn copy_skill_rendered(
    id: &std::ffi::OsStr,
    src: &std::path::Path,
    dst: &std::path::Path,
) -> Result<(), String> {
    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())?.flatten() {
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            std::fs::create_dir_all(&dst_path).map_err(|e| e.to_string())?;
            copy_skill_rendered(id, &src_path, &dst_path)?;
            continue;
        }
        match std::fs::read_to_string(&src_path) {
            Ok(text) => std::fs::write(&dst_path, render_bundled_skill(id, &text))
                .map_err(|e| e.to_string())?,
            Err(_) => {
                std::fs::copy(&src_path, &dst_path).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

/// Mirror CodeTerm's owned skill entries into provider skill roots.
pub fn mirror_skills_to_providers() {
    let canonical = crate::paths::data_dir().join("skills");
    let Some(home) = dirs::home_dir() else {
        return;
    };
    let targets = [
        home.join(".claude").join("skills"),
        home.join(".agents").join("skills"),
    ];
    mirror_into_providers_if_allowed(
        &canonical,
        &targets,
        codeterm_plugin::global_writes::machine_global_write_allowed(),
    );
}

fn mirror_into_providers_if_allowed(
    canonical: &std::path::Path,
    targets: &[std::path::PathBuf],
    allowed: bool,
) {
    if !allowed {
        log::info!("[skills] skipped global provider mirror for non-owner instance");
        return;
    }
    mirror_into_providers(canonical, targets);
}

/// Pure inner implementation — injectable paths make this testable without touching real home dirs.
fn mirror_into_providers(canonical: &std::path::Path, targets: &[std::path::PathBuf]) {
    let codeterm_src = canonical.join(CODETERM_SKILL_ID);
    let codeterm_id = std::ffi::OsStr::new(CODETERM_SKILL_ID);
    for target in targets {
        let codeterm_dst = target.join(CODETERM_SKILL_ID);
        let source_revision = read_skill_revision(&codeterm_src);
        if (!codeterm_dst.exists() || provider_entry_owned(&codeterm_dst))
            && should_write_provider_entry(&codeterm_src, &codeterm_dst, source_revision)
        {
            let _ = std::fs::create_dir_all(&codeterm_dst);
            remove_unbundled_entries(&codeterm_src, &codeterm_dst, true);
            match copy_skill_rendered(codeterm_id, &codeterm_src, &codeterm_dst) {
                Ok(()) => {
                    let _ = write_provider_ownership(&codeterm_dst, source_revision);
                    log::info!("[skills] mirrored codeterm -> {}", codeterm_dst.display());
                }
                Err(e) => log::warn!(
                    "[skills] mirror codeterm -> {} failed: {}",
                    codeterm_dst.display(),
                    e
                ),
            }
        }

        let Ok(entries) = std::fs::read_dir(target) else {
            continue;
        };
        for entry in entries.flatten() {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let path = entry.path();
            if path.file_name() == Some(codeterm_id) {
                continue;
            }
            if !provider_entry_prunable(&path) {
                continue;
            }
            log::info!("[skills] pruning owned provider entry: {}", path.display());
            if let Err(e) = std::fs::remove_dir_all(&path) {
                log::warn!("[skills] failed to remove {}: {}", path.display(), e);
            }
        }
    }
}

fn provider_entry_owned(path: &std::path::Path) -> bool {
    provider_metadata(path).is_some()
}

fn provider_entry_prunable(path: &std::path::Path) -> bool {
    let source_channel = if cfg!(debug_assertions) { "dev" } else { "prod" };
    provider_metadata(path).is_some_and(|(_, installed_channel, _)| {
        installed_channel != "prod" || source_channel == "prod"
    })
}

fn read_skill_revision(path: &std::path::Path) -> u32 {
    std::fs::read_to_string(path.join("SKILL.md"))
        .map(|content| extract_revision(&content))
        .unwrap_or_default()
}

fn provider_metadata(path: &std::path::Path) -> Option<(u32, String, String)> {
    let data = std::fs::read_to_string(path.join(PROVIDER_OWNERSHIP_FILE)).ok()?;
    let mut lines = data.lines();
    if lines.next()? != "codeterm" {
        return None;
    }
    let revision = lines.next()?.parse().ok()?;
    let channel = lines.next()?.to_string();
    let hash = lines.next()?.to_string();
    if !matches!(channel.as_str(), "dev" | "prod")
        || hash.len() != 64
        || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    Some((revision, channel, hash))
}

fn should_write_provider_entry(src: &std::path::Path, dst: &std::path::Path, revision: u32) -> bool {
    if !dst.exists() {
        return true;
    }
    let source_hash = provider_skill_hash(src, true);
    let Some((installed_revision, installed_channel, installed_hash)) = provider_metadata(dst) else {
        return false;
    };
    let source_channel = if cfg!(debug_assertions) { "dev" } else { "prod" };
    if installed_channel == "prod" && source_channel == "dev" {
        return false;
    }
    if installed_channel == source_channel && installed_revision > revision {
        return false;
    }
    installed_hash != source_hash
        || provider_skill_hash(dst, false) != installed_hash
        || installed_revision != revision
        || installed_channel != source_channel
}

fn provider_skill_hash(path: &std::path::Path, render: bool) -> String {
    use sha2::Digest;
    let mut files = Vec::new();
    fn collect(path: &std::path::Path, root: &std::path::Path, files: &mut Vec<(String, Vec<u8>)>) {
        let Ok(entries) = std::fs::read_dir(path) else { return };
        for entry in entries.flatten() {
            let item = entry.path();
            if item.is_dir() {
                collect(&item, root, files);
            } else if item.file_name().is_some_and(|name| name != PROVIDER_OWNERSHIP_FILE) {
                if let Ok(mut bytes) = std::fs::read(&item) {
                    if render && item == root.join("SKILL.md") {
                        if let Ok(text) = String::from_utf8(bytes.clone()) {
                            bytes = render_bundled_skill(
                                std::ffi::OsStr::new(CODETERM_SKILL_ID),
                                &text,
                            )
                            .into_bytes();
                        }
                    }
                    files.push((item.strip_prefix(root).unwrap_or(&item).display().to_string(), bytes));
                }
            }
        }
    }
    collect(path, path, &mut files);
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut digest = sha2::Sha256::new();
    for (name, bytes) in files {
        digest.update(name.as_bytes());
        digest.update([0]);
        digest.update(bytes);
    }
    format!("{:x}", digest.finalize())
}

fn remove_unbundled_entries(src: &std::path::Path, dst: &std::path::Path, root: bool) {
    let Ok(entries) = std::fs::read_dir(dst) else {
        return;
    };
    for entry in entries.flatten() {
        let destination = entry.path();
        let name = entry.file_name();
        if root && name == PROVIDER_OWNERSHIP_FILE {
            continue;
        }
        let source = src.join(&name);
        if !source.exists() || source.is_dir() != destination.is_dir() {
            if destination.is_dir() {
                let _ = std::fs::remove_dir_all(destination);
            } else {
                let _ = std::fs::remove_file(destination);
            }
        } else if source.is_dir() && destination.is_dir() {
            remove_unbundled_entries(&source, &destination, false);
        }
    }
}

fn write_provider_ownership(path: &std::path::Path, revision: u32) -> Result<(), String> {
    let channel = if cfg!(debug_assertions) { "dev" } else { "prod" };
    std::fs::write(
        path.join(PROVIDER_OWNERSHIP_FILE),
        format!("codeterm\n{revision}\n{channel}\n{}\n", provider_skill_hash(path, false)),
    )
    .map_err(|error| error.to_string())
}

/// Copy if the provider copy is missing, or differs from canonical anywhere in the
/// skill directory — a changed `references/` file counts. Never downgrades.
fn should_mirror_skill(src_dir: &std::path::Path, dst_dir: &std::path::Path) -> bool {
    should_copy_skill(std::ffi::OsStr::new(CODETERM_SKILL_ID), src_dir, dst_dir)
}

/// Task-brief block for a cron job's `--skill <slug>`: the exact `SKILL.md`
/// path to Read, resolved against the canonical dir (never provider dirs).
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
mod skills_seed_tests {
    use super::*;

    const BUNDLED_SKILL: &str = concat!(
        "# CodeTerm\n\n",
        "{codeterm_cli}\n\n",
        "{interactive_child_pty_operation}\n\n",
        "---\n\n",
        "{agent_pad_rule}\n\n",
        "<!-- revision: 7 -->\n",
    );

    /// A bundled fixture carrying its render slots, plus one `references/` file
    /// so the directory-scoped rungs have something to compare beyond SKILL.md.
    fn bundled_fixture(root: &std::path::Path, skill_body: &str, reference: &str) {
        let dir = root.join(CODETERM_SKILL_ID);
        std::fs::create_dir_all(dir.join("references")).unwrap();
        std::fs::write(dir.join("SKILL.md"), skill_body).unwrap();
        std::fs::write(dir.join("references").join("mem.md"), reference).unwrap();
    }

    fn canonical_skill(canonical: &std::path::Path) -> String {
        std::fs::read_to_string(canonical.join(CODETERM_SKILL_ID).join("SKILL.md")).unwrap()
    }

    /// Round-trip through substitution: the rule arrives because the constant was
    /// substituted, and no placeholder residue survives. The fragment is an opaque
    /// token here — its wording belongs to `agent_pad_rule.md`.
    #[test]
    fn seed_renders_shared_codeterm_fragments_from_constants() {
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let canonical = tmp.path().join("canonical");
        bundled_fixture(&bundled, BUNDLED_SKILL, "mem reference v1");

        seed_bundled_skills_into(&bundled, &canonical);

        let rendered = canonical_skill(&canonical);
        let rule = crate::prompts::AGENT_PAD_RULE.trim();
        assert!(!rule.is_empty(), "an empty fragment proves nothing");
        assert_eq!(
            rendered.matches(rule).count(),
            1,
            "the rendered skill must carry the AgentPad rule exactly once"
        );
        assert_eq!(rendered.matches(crate::prompts::CODETERM_CLI).count(), 1);
        assert_eq!(
            rendered
                .matches(crate::prompts::INTERACTIVE_CHILD_PTY_OPERATION.trim())
                .count(),
            1
        );
        for (slot, _) in codeterm_skill_render_map() {
            assert!(
                !rendered.contains(slot),
                "unsubstituted placeholder remains: {slot}"
            );
        }

        // Gate and copy must agree, or every start reseeds the same bytes.
        assert!(
            !should_copy_skill(
                std::ffi::OsStr::new(CODETERM_SKILL_ID),
                &bundled.join(CODETERM_SKILL_ID),
                &canonical.join(CODETERM_SKILL_ID),
            ),
            "a freshly seeded tree must compare equal to its rendered source"
        );
    }

    /// The gate, not the copy. Revisions are equal by construction, so a
    /// revision-only gate skips and the stale rendering survives; only a
    /// render-aware content compare reseeds.
    #[test]
    fn fragment_edit_reseeds_at_same_revision() {
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let canonical = tmp.path().join("canonical");
        bundled_fixture(&bundled, BUNDLED_SKILL, "mem reference v1");
        seed_bundled_skills_into(&bundled, &canonical);
        let fresh = canonical_skill(&canonical);

        // Same revision, rendering from an older fragment.
        let stale = fresh.replace(crate::prompts::AGENT_PAD_RULE.trim(), "STALE_FRAGMENT");
        assert_ne!(stale, fresh, "the fixture must actually go stale");
        let dst_skill = canonical.join(CODETERM_SKILL_ID).join("SKILL.md");
        std::fs::write(&dst_skill, &stale).unwrap();
        assert_eq!(
            extract_revision(&std::fs::read_to_string(&dst_skill).unwrap()),
            extract_revision(
                &std::fs::read_to_string(bundled.join(CODETERM_SKILL_ID).join("SKILL.md")).unwrap()
            ),
            "the equal-revision branch is what this test exercises"
        );

        seed_bundled_skills_into(&bundled, &canonical);

        assert_eq!(canonical_skill(&canonical), fresh);
        assert!(!canonical_skill(&canonical).contains("STALE_FRAGMENT"));
    }

    /// SKILL.md is identical, so the earlier rung is satisfied-equal by
    /// construction and only a directory-scoped compare reaches the changed
    /// reference file.
    #[test]
    fn mirror_fires_on_reference_change_at_equal_revision() {
        let tmp = tempfile::tempdir().unwrap();
        let canonical = tmp.path().join("canonical");
        let provider = tmp.path().join("provider");
        let skill = "# CodeTerm\n\nno placeholders\n\n<!-- revision: 7 -->\n";
        bundled_fixture(&canonical, skill, "mem reference v2");
        bundled_fixture(&provider, skill, "mem reference v1");

        let src_dir = canonical.join(CODETERM_SKILL_ID);
        let dst_dir = provider.join(CODETERM_SKILL_ID);
        let channel = if cfg!(debug_assertions) { "dev" } else { "prod" };
        let hash = provider_skill_hash(&dst_dir, false);
        std::fs::write(
            dst_dir.join(PROVIDER_OWNERSHIP_FILE),
            format!("codeterm\n7\n{channel}\n{hash}\n"),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(src_dir.join("SKILL.md")).unwrap(),
            std::fs::read_to_string(dst_dir.join("SKILL.md")).unwrap(),
            "SKILL.md must be equal so the later rung is the one under test"
        );
        assert!(should_mirror_skill(&src_dir, &dst_dir));

        mirror_into_providers(&canonical, &[provider.clone()]);

        assert_eq!(
            std::fs::read_to_string(dst_dir.join("references").join("mem.md")).unwrap(),
            "mem reference v2"
        );
        assert!(!should_mirror_skill(&src_dir, &dst_dir));
    }

    /// Never-downgrade stays the revision tag's only job.
    #[test]
    fn seed_never_downgrades_a_newer_canonical_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let canonical = tmp.path().join("canonical");
        bundled_fixture(&bundled, BUNDLED_SKILL, "mem reference v1");
        let newer = "# Newer\n\nhand-edited\n\n<!-- revision: 99 -->\n";
        bundled_fixture(&canonical, newer, "mem reference newer");

        seed_bundled_skills_into(&bundled, &canonical);

        assert_eq!(canonical_skill(&canonical), newer);
    }

    /// A non-`codeterm` skill has no render map: its bytes seed verbatim,
    /// placeholder-looking text included.
    #[test]
    fn other_bundled_skills_are_copied_without_substitution() {
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let canonical = tmp.path().join("canonical");
        let body = "# Other\n\n{codeterm_cli}\n\n<!-- revision: 1 -->\n";
        std::fs::create_dir_all(bundled.join("other")).unwrap();
        std::fs::write(bundled.join("other").join("SKILL.md"), body).unwrap();

        seed_bundled_skills_into(&bundled, &canonical);

        assert_eq!(
            std::fs::read_to_string(canonical.join("other").join("SKILL.md")).unwrap(),
            body
        );
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

        assert!(
            should_mirror_skill(&src_dir, &dst_dir),
            "missing provider copy -> mirror"
        );

        write_skill(&dst_dir, "a\n<!-- revision: 5 -->");
        assert!(
            !should_mirror_skill(&src_dir, &dst_dir),
            "identical -> skip"
        );

        write_skill(&dst_dir, "OLD\n<!-- revision: 5 -->");
        assert!(
            should_mirror_skill(&src_dir, &dst_dir),
            "same rev, changed content -> mirror"
        );

        write_skill(&dst_dir, "x\n<!-- revision: 9 -->");
        assert!(
            !should_mirror_skill(&src_dir, &dst_dir),
            "provider newer -> never downgrade"
        );
    }

    #[test]
    fn skills_mirror_prunes_only_owned_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let canonical = tmp.path().join("canonical");
        let provider = tmp.path().join("provider");

        // Canonical has codeterm + another skill (atlassian stays in canonical only).
        write_skill(
            &canonical.join("codeterm"),
            "codeterm skill body\n<!-- revision: 2 -->",
        );
        write_skill(&canonical.join("atlassian"), "atlassian skill body");

        write_skill(&provider.join("synced"), "provider-managed");
        write_skill(&provider.join("qa-foreign"), "foreign");
        write_skill(&provider.join("codeterm-owned-old"), "obsolete");
        std::fs::write(
            provider.join("codeterm-owned-old").join(PROVIDER_OWNERSHIP_FILE),
            "codeterm\n1\ndev\n0000000000000000000000000000000000000000000000000000000000000000\n",
        )
        .unwrap();
        write_skill(&provider.join("codeterm"), "stale owned copy");
        std::fs::write(
            provider.join("codeterm").join(PROVIDER_OWNERSHIP_FILE),
            "codeterm\n1\ndev\n0000000000000000000000000000000000000000000000000000000000000000\n",
        )
        .unwrap();

        mirror_into_providers(&canonical, &[provider.clone()]);

        assert!(
            provider.join("codeterm").join("SKILL.md").exists(),
            "codeterm SKILL.md must be present after mirror"
        );
        assert!(provider.join("synced").exists());
        assert!(provider.join("qa-foreign").exists());
        assert!(!provider.join("codeterm-owned-old").exists());
        assert!(
            provider.join("codeterm").join(PROVIDER_OWNERSHIP_FILE).exists(),
            "owned marker must remain inside the mirrored entry"
        );
        assert!(
            !provider.join("atlassian").exists(),
            "non-codeterm canonical skill must not appear in provider dir"
        );
    }

    #[test]
    fn dev_instance_skips_machine_global_writes() {
        let tmp = tempfile::tempdir().unwrap();
        let canonical = tmp.path().join("canonical");
        let provider = tmp.path().join("provider");
        write_skill(&canonical.join("codeterm"), "new global skill");
        write_skill(&provider.join("codeterm"), "existing bytes");
        let before = std::fs::read(provider.join("codeterm").join("SKILL.md")).unwrap();

        mirror_into_providers_if_allowed(&canonical, &[provider.clone()], false);

        assert_eq!(
            std::fs::read(provider.join("codeterm").join("SKILL.md")).unwrap(),
            before
        );
    }

    #[test]
    fn global_write_only_on_content_change() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source");
        let target = tmp.path().join("target");
        write_skill(&source, "body\n<!-- revision: 5 -->");
        write_skill(&target, "body\n<!-- revision: 5 -->");
        let channel = if cfg!(debug_assertions) { "dev" } else { "prod" };
        let hash = provider_skill_hash(&target, false);
        std::fs::write(
            target.join(PROVIDER_OWNERSHIP_FILE),
            format!("codeterm\n5\n{channel}\n{hash}\n"),
        )
        .unwrap();
        assert!(!should_write_provider_entry(&source, &target, 5));
        std::fs::write(target.join(PROVIDER_OWNERSHIP_FILE), format!("codeterm\n5\n{channel}\n0000000000000000000000000000000000000000000000000000000000000000\n")).unwrap();
        assert!(should_write_provider_entry(&source, &target, 5));
        std::fs::write(target.join(PROVIDER_OWNERSHIP_FILE), "codeterm\n99\nprod\n0000000000000000000000000000000000000000000000000000000000000000\n").unwrap();
        assert!(!should_write_provider_entry(&source, &target, 5));
    }
}
