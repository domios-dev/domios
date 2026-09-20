"""Structural contract for the single-run release workflow graph."""

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WORKFLOWS = ROOT / ".github" / "workflows"
VERIFIER = "scripts/verify-updater-manifest.mjs"
RELEASE_NOTES_SYNC = "scripts/sync-release-notes.mjs"
PLATFORMS = {
    "darwin-aarch64",
    "darwin-x86_64",
    "windows-x86_64",
    "windows-aarch64",
    "linux-x86_64",
    "linux-aarch64",
}

STEP_START = re.compile(r"^ {6}-\s+(.*)$")
STEP_KEY = re.compile(r"^ {8}([A-Za-z_][A-Za-z0-9_-]*):\s?(.*)$")
INLINE_KEY = re.compile(r"^([A-Za-z_][A-Za-z0-9_-]*):\s?(.*)$")
TRIGGER_KEY = re.compile(r"^  ([A-Za-z_][A-Za-z0-9_]*):")
ON_HEADER = re.compile(r'^"?on"?:\s*$')
BLOCK_SCALAR = {"|", ">", "|-", ">-", "|+", ">+"}
REQUIRE_PLATFORM = re.compile(r"--require-platform\s+([A-Za-z0-9_-]+)")


def read(name: str) -> str:
    return (WORKFLOWS / name).read_text(encoding="utf-8")


def triggers(text: str) -> set[str]:
    lines = text.splitlines()
    for index, line in enumerate(lines):
        if ON_HEADER.match(line):
            found = set()
            for candidate in lines[index + 1 :]:
                if candidate.strip() == "":
                    continue
                if not candidate.startswith("  "):
                    break
                match = TRIGGER_KEY.match(candidate)
                if match:
                    found.add(match.group(1))
            return found
    raise AssertionError("workflow declares no 'on:' block")


def _block_body(lines: list[str], start: int) -> tuple[str, int]:
    body: list[str] = []
    index = start
    while index < len(lines):
        line = lines[index]
        if line.strip() == "":
            body.append("")
            index += 1
            continue
        if len(line) - len(line.lstrip()) < 10:
            break
        body.append(line[10:])
        index += 1
    return "\n".join(body), index


def steps(text: str) -> list[dict[str, str]]:
    lines = text.splitlines()
    parsed: list[dict[str, str]] = []
    index = 0
    while index < len(lines):
        head = STEP_START.match(lines[index])
        if not head:
            index += 1
            continue
        step: dict[str, str] = {}
        inline = INLINE_KEY.match(head.group(1))
        if inline:
            step[inline.group(1)] = inline.group(2)
        index += 1
        while index < len(lines):
            line = lines[index]
            if line.strip() == "":
                index += 1
                continue
            if len(line) - len(line.lstrip()) <= 6:
                break
            key = STEP_KEY.match(line)
            if key and key.group(2).strip() in BLOCK_SCALAR:
                step[key.group(1)], index = _block_body(lines, index + 1)
                continue
            if key:
                step[key.group(1)] = key.group(2)
            index += 1
        parsed.append(step)
    return parsed


def verifier_steps(text: str) -> list[dict[str, str]]:
    return [step for step in steps(text) if VERIFIER in step.get("run", "")]


class SingleRunGraphTest(unittest.TestCase):
    def test_only_the_caller_has_the_release_tag_trigger(self):
        triggered = {
            path.name
            for path in WORKFLOWS.glob("release*.yml")
            if "push" in triggers(path.read_text(encoding="utf-8"))
        }
        self.assertEqual(triggered, {"release.yml"})
        for name in (
            "release-builder.yml",
            "release-linux.yml",
            "release-windows.yml",
            "release-manifest-validate.yml",
        ):
            self.assertEqual(triggers(read(name)), {"workflow_call"}, name)

    def test_platform_jobs_run_in_parallel_before_the_final_manifest_job(self):
        caller = read("release.yml")
        for job, workflow in (
            ("macos-linux-arm64", "release-builder.yml"),
            ("linux-x86_64", "release-linux.yml"),
            ("windows", "release-windows.yml"),
        ):
            self.assertIn(f"  {job}:\n    name:", caller)
            self.assertIn("    needs: prepare", caller[caller.index(f"  {job}:") :])
            self.assertIn(f"uses: ./.github/workflows/{workflow}", caller)

        final = caller[caller.index("  release-manifest:") :]
        self.assertIn(
            "needs: [prepare, macos-linux-arm64, linux-x86_64, windows]", final
        )
        self.assertIn("uses: ./.github/workflows/release-manifest-validate.yml", final)
        self.assertNotIn("if: always()", final)

    def test_preparation_creates_drafts_and_cleans_assets_before_parallel_uploads(self):
        caller = read("release.yml")
        prepare = caller[caller.index("  prepare:") : caller.index("  macos-linux-arm64:")]
        self.assertIn("gh release create", prepare)
        self.assertIn("--draft", prepare)
        self.assertIn("scripts/release-clear-wedged-assets.mjs", prepare)
        self.assertIn("rollacode/codeterm-releases", prepare)

    def test_reusable_platform_jobs_can_clear_incomplete_uploads_when_retried_alone(self):
        for name, build_marker in (
            ("release-builder.yml", "Build macOS + Linux"),
            ("release-linux.yml", "Build release bundle inputs"),
            ("release-windows.yml", "Build release bundle inputs"),
        ):
            with self.subTest(workflow=name):
                text = read(name)
                clear = text.index("Clear incomplete release uploads for retry")
                build = text.index(build_marker)
                self.assertLess(clear, build)
                self.assertIn("GH_RELEASE_TOKEN", text[clear:build])
                self.assertIn("release-clear-wedged-assets.mjs", text[clear:build])
                self.assertIn("rollacode/codeterm-releases", text[clear:build])

    def test_reusable_platform_jobs_keep_named_builds_and_artifact_fragments(self):
        mac = read("release-builder.yml")
        linux = read("release-linux.yml")
        windows = read("release-windows.yml")
        self.assertIn("name: macOS + Linux arm64", mac)
        self.assertIn("CT_BUILD_LINUX: '1'", mac)
        self.assertIn("DEFER_RELEASE_FINALIZE: '1'", mac)
        self.assertIn("name: build (x86_64-unknown-linux-gnu)", linux)
        self.assertIn("Publish AppImage updater fragment", linux)
        self.assertIn("Publish Debian updater fragment", linux)
        self.assertIn("bundles deb,appimage", linux)
        self.assertIn("name: build (${{ matrix.target }})", windows)
        self.assertIn("windows-x86_64", windows)
        self.assertIn("windows-aarch64", windows)
        self.assertIn("PLATFORM_KEY: ${{ matrix.platform_key }}", windows)

    def test_platform_packaging_and_signing_stay_on_the_owning_platform(self):
        mac = read("release-builder.yml")
        linux = read("release-linux.yml")
        windows = read("release-windows.yml")
        self.assertIn("APPLE_APP_SPECIFIC_PASSWORD", mac)
        self.assertIn("apple-actions/import-codesign-certs", read("release-macos.yml"))
        self.assertIn("bundles deb,appimage", linux)
        self.assertIn("bundleRoot/nsis", windows)
        self.assertNotIn("bundleRoot/nsis", linux)
        self.assertNotIn("bundle/deb", windows)

    def test_manifest_is_reconciled_once_after_platform_uploads(self):
        manifest = read("release-manifest-validate.yml")
        self.assertEqual(triggers(manifest), {"workflow_call"})
        self.assertNotIn("workflow_run", manifest)
        self.assertIn("Merge and upload final channel manifests", manifest)
        self.assertIn("Mark source and public releases published", manifest)
        self.assertIn("sync-release-notes.mjs", manifest)
        self.assertIn("latest-deb.json", manifest)
        self.assertNotIn("release-completion", manifest)
        self.assertEqual(len(verifier_steps(manifest)), 2)
        covered = set().union(
            *(set(REQUIRE_PLATFORM.findall(step["run"])) for step in verifier_steps(manifest))
        )
        self.assertTrue(PLATFORMS <= covered)

    def test_manifest_and_release_closure_follow_public_byte_verification(self):
        parsed = steps(read("release-manifest-validate.yml"))
        order = [step.get("name") for step in parsed]
        self.assertLess(
            order.index("Merge and upload final channel manifests"),
            order.index("Mark source and public releases published"),
        )
        self.assertLess(
            order.index("Sync full release notes to source and public mirror"),
            order.index("Verify published public mirror"),
        )
        verify = next(step for step in parsed if step.get("name") == "Verify published public mirror")
        self.assertIn("?cb=$(date +%s)", verify["run"])
        self.assertIn("--require-version", verify["run"])
        close = next(step for step in parsed if step.get("name") == "Close shipped public issues")
        self.assertIn("check-release-closure.mjs", close["run"])
        self.assertLess(close["run"].index("check-release-closure.mjs"), close["run"].index("gh issue close"))

    def test_release_names_notes_sync_is_final_and_build_cache_stays_unshared(self):
        caller = read("release.yml")
        self.assertIn("release-manifest:", caller)
        lanes = ("release-builder.yml", "release-linux.yml", "release-windows.yml")
        for name in lanes:
            text = read(name)
            self.assertNotRegex(text, r"(?i)rust-cache|swatinem")
            if name != "release-builder.yml":
                values = re.findall(r"CARGO_TARGET_DIR:\s*(\S.*)$", text, re.MULTILINE)
                self.assertTrue(values)
                self.assertTrue(all(value.startswith("${{ github.workspace }}/") for value in values))

        self.assertNotIn(RELEASE_NOTES_SYNC, read("release-builder.yml"))
        self.assertIn(RELEASE_NOTES_SYNC, read("release-manifest-validate.yml"))

    def test_release_input_prebuilds_app_and_embed_bins_together(self):
        build = (ROOT / "build.sh").read_text(encoding="utf-8")
        self.assertIn('cargo_target_args=(--target "$CARGO_BUILD_TARGET")', build)
        self.assertIn('--features "${CODETERM_RELEASE_FEATURES:-embeddings}"', build)
        self.assertIn("--bin codeterm --bin codeterm-embed", build)

    def test_linux_and_windows_keep_the_release_build_contract(self):
        for name in ("release-linux.yml", "release-windows.yml"):
            with self.subTest(workflow=name):
                text = read(name)
                self.assertIn("CARGO_TARGET_DIR: ${{ github.workspace }}/src-tauri/target", text)
                self.assertIn("CARGO_INCREMENTAL: '0'", text)
                self.assertIn("CODETERM_RELEASE_FEATURES: embeddings", text)
                self.assertIn(
                    "args: --target ${{ env.TARGET }} --features ${{ env.CODETERM_RELEASE_FEATURES }}",
                    text,
                )

    def test_platform_specific_packaging_and_macos_notarization_stay_with_the_owner(self):
        linux = read("release-linux.yml")
        windows = read("release-windows.yml")
        mac = read("release-builder.yml")
        self.assertIn("--bundles deb,appimage", linux)
        self.assertIn("bundleRoot/nsis", windows)
        self.assertNotIn("bundleRoot/nsis", linux)
        self.assertNotIn("bundle/deb", windows)
        self.assertIn("build-all.sh", mac)

        overlap = (ROOT / ".codeterm" / "build-machine" / "scripts" / "mac-overlap.sh").read_text(
            encoding="utf-8"
        )
        self.assertIn('notarize-dmg.sh" "$ARM_DMG"', overlap)
        self.assertIn("ARM_NOTARY_PID=$!", overlap)
        self.assertIn('mac-bundle.sh" "$VER" x86_64', overlap)
        self.assertIn('wait "$ARM_NOTARY_PID"', overlap)

    def test_manifest_release_gate_follows_all_parallel_jobs(self):
        caller = read("release.yml")
        platform_jobs = [caller.index(f"  {name}") for name in ("macos-linux-arm64:", "linux-x86_64:", "windows:")]
        manifest_job = caller.index("  release-manifest:")
        self.assertEqual(platform_jobs, sorted(platform_jobs))
        self.assertGreater(manifest_job, max(platform_jobs))
        self.assertIn(
            "needs: [prepare, macos-linux-arm64, linux-x86_64, windows]",
            caller[manifest_job:],
        )

    def test_mac_pipeline_keeps_notarization_overlapped_with_the_other_arch(self):
        script = (ROOT / ".codeterm" / "build-machine" / "scripts" / "mac-overlap.sh").read_text(
            encoding="utf-8"
        )
        self.assertIn('notarize-dmg.sh" "$ARM_DMG"', script)
        self.assertIn("ARM_NOTARY_PID=$!", script)
        self.assertIn('mac-bundle.sh" "$VER" x86_64', script)
        self.assertIn('wait "$ARM_NOTARY_PID"', script)


if __name__ == "__main__":
    unittest.main()
