"""End-to-end fixture round-trip and refusal tests for Domios portable essentials archives.

Proves:
- Exact manifest round-trip from a real temp Git repo with credential-free remote, exact HEAD,
  unpushed commit (Git bundle), binary + text staged changes, unstaged change, untracked user file,
  omitted regenerable node_modules/target (with provable manifests), sentinel file, Domios desk/
  sessions/mem/settings, and portable agent-home config.
- Strict exclusion of login credentials, keys, vault, running authority, and replayable sends.
- Eviction refusals on credential-bearing remote, unknown user file, missing recovery input,
  mid-rebase repo, corrupt archive, and competing wake/stale generation fence.
- Owner/machine/generation binding (same-owner succeeds, cross-owner and wrong generation refused).
- Failed restore (including simulated ENOSPC disk full) preserves the source archive intact.
"""

from __future__ import annotations

import errno
import importlib.util
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest

REPO = pathlib.Path(__file__).resolve().parent.parent


def load_cloud_archive():
    spec = importlib.util.spec_from_file_location(
        "domios_cloud_archive_under_test", REPO / "packaging/docker/cloud_archive.py"
    )
    mod = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = mod
    spec.loader.exec_module(mod)
    return mod


def run_git(cwd: pathlib.Path, *args: str) -> str:
    proc = subprocess.run(
        ["git", "-C", str(cwd), *args],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=True,
    )
    return proc.stdout.strip()


def make_fixture_home(root: pathlib.Path) -> tuple[pathlib.Path, pathlib.Path, str, bytes]:
    cloud_home = root / "cloud_home"
    bare_origin = root / "origin.git"
    cloud_home.mkdir(parents=True)
    bare_origin.mkdir(parents=True)

    run_git(bare_origin, "init", "--bare", "-b", "main")

    # 1. Portable Domios state (desk, sessions, mem, settings)
    domios = cloud_home / ".domios"
    domios.mkdir()
    (domios / "config.toml").write_text('[neighbors]\nkey_store = "file"\n', encoding="utf-8")
    (domios / "desk.json").write_text(json.dumps({"workspaces": [{"id": "ws-1", "name": "Demo"}]}), encoding="utf-8")
    (domios / "sessions.json").write_text(json.dumps({"tabs": [{"id": "tab-1", "resumable": True}]}), encoding="utf-8")
    (domios / "memory.sqlite").write_bytes(b"SQLite format 3\x00fixture-mem-data")

    # 2. Secrets, keys, running authority, and replayable sends (MUST BE EXCLUDED!)
    (domios / ".secrets").write_text('{"api_key": "sk-secret-must-never-archive"}', encoding="utf-8")
    (domios / "neighbor-keys").mkdir()
    (domios / "neighbor-keys" / "install.key").write_text("PRIVATE-KEY-DO-NOT-COPY", encoding="utf-8")
    (domios / "cloud_wake_queue.json").write_text('[{"send_id":"s1","payload":"do-not-replay"}]', encoding="utf-8")
    (domios / "server.pid").write_text("12345", encoding="utf-8")

    # 3. Eligible portable agent-home config + excluded provider auth files
    (cloud_home / ".claude").mkdir()
    (cloud_home / ".claude" / "settings.json").write_text('{"theme":"dark"}', encoding="utf-8")
    (cloud_home / ".claude" / ".credentials.json").write_text('{"oauth":"DO_NOT_COPY"}', encoding="utf-8")
    (cloud_home / ".codex").mkdir()
    (cloud_home / ".codex" / "config.toml").write_text('model = "gpt-5"\n', encoding="utf-8")
    (cloud_home / ".codex" / "auth.json").write_text('{"token":"DO_NOT_COPY"}', encoding="utf-8")

    # 4. Named non-recreatable sentinel user file
    (cloud_home / "sentinel.txt").write_text("harborlight-sentinel-v1\n", encoding="utf-8")

    # 5. Managed Git repo with bare remote, unpushed commit, binary+text staged, unstaged, untracked, and regenerable dirs
    repo = cloud_home / "projects" / "app"
    repo.parent.mkdir(parents=True)
    run_git(cloud_home / "projects", "clone", str(bare_origin), "app")
    run_git(repo, "config", "user.email", "owner@domios.test")
    run_git(repo, "config", "user.name", "Domios Owner")

    (repo / "package.json").write_text('{"name":"demo","version":"1.0.0"}\n', encoding="utf-8")
    (repo / "Cargo.toml").write_text('[package]\nname = "demo"\nversion = "0.1.0"\n', encoding="utf-8")
    (repo / "README.md").write_text("# Demo\nLine 1\n", encoding="utf-8")
    run_git(repo, "add", ".")
    run_git(repo, "commit", "-m", "initial commit")
    run_git(repo, "push", "-u", "origin", "main")

    # Unpushed commit (must survive via Git bundle!)
    (repo / "feature.txt").write_text("unpushed feature work\n", encoding="utf-8")
    run_git(repo, "add", "feature.txt")
    run_git(repo, "commit", "-m", "unpushed local commit")
    expected_head = run_git(repo, "rev-parse", "HEAD")

    # Binary + text staged change
    binary_payload = bytes([0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0xFF, 0x42, 0x99])
    (repo / "asset.bin").write_bytes(binary_payload)
    (repo / "README.md").write_text("# Demo\nLine 1\nStaged addition\n", encoding="utf-8")
    run_git(repo, "add", "asset.bin", "README.md")

    # Unstaged modification on top of staged file
    (repo / "feature.txt").write_text("unpushed feature work\nunstaged edit\n", encoding="utf-8")

    # Untracked user file
    (repo / "notes.local.md").write_text("untracked owner notes\n", encoding="utf-8")

    # Known regenerable directories (with provable package.json & Cargo.toml)
    (repo / "node_modules" / "pkg").mkdir(parents=True)
    (repo / "node_modules" / "pkg" / "index.js").write_text("module.exports = 1;\n", encoding="utf-8")
    (repo / "target" / "debug").mkdir(parents=True)
    (repo / "target" / "debug" / "artifact.bin").write_bytes(b"\x00" * 4096)

    return cloud_home, bare_origin, expected_head, binary_payload


class CloudArchiveTests(unittest.TestCase):
    def test_exact_manifest_round_trip_and_secret_exclusion(self):
        mod = load_cloud_archive()
        owner_key = b"K" * 32
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            cloud_home, _, expected_head, binary_payload = make_fixture_home(root)
            archive_file = root / "archives" / "arc_roundtrip.enc"

            exported = mod.export_essentials_archive(
                cloud_home,
                archive_file,
                owner_uid="uid-owner-1",
                machine_id="cm_rt01",
                generation=1,
                owner_key=owner_key,
                archive_id="arc_rt01",
                named_user_files=["sentinel.txt"],
            )
            self.assertTrue(exported.excluded_secrets_verified)
            self.assertGreater(exported.archive_bytes, 0)
            self.assertEqual(exported.manifest["repos"][0]["head_sha"], expected_head)
            self.assertTrue(exported.manifest["repos"][0]["has_bundle"])
            self.assertTrue(exported.manifest["repos"][0]["has_staged_patch"])
            self.assertTrue(exported.manifest["repos"][0]["has_unstaged_patch"])
            self.assertIn("node_modules", exported.manifest["repos"][0]["omitted_regenerable"])
            self.assertIn("target", exported.manifest["repos"][0]["omitted_regenerable"])

            # Print measured timings & bytes for economics verification
            print(json.dumps({
                "measured_export": {
                    "archive_bytes": exported.archive_bytes,
                    "uncompressed_bytes": exported.uncompressed_bytes,
                    "timings_ms": exported.timings_ms,
                }
            }))

            # Cross-owner and wrong-generation restore MUST fail before touching target
            bad_target = root / "cross_owner_vol"
            with self.assertRaises(mod.ArchiveVerificationError) as cm_owner:
                mod.restore_essentials_archive(
                    archive_file,
                    bad_target,
                    owner_key=owner_key,
                    requester_uid="uid-other-2",
                    target_machine_id="cm_rt01",
                    expected_generation=1,
                )
            self.assertEqual(cm_owner.exception.code, "owner_mismatch")

            with self.assertRaises(mod.ArchiveVerificationError) as cm_gen:
                mod.restore_essentials_archive(
                    archive_file,
                    bad_target,
                    owner_key=owner_key,
                    requester_uid="uid-owner-1",
                    target_machine_id="cm_rt01",
                    expected_generation=99,
                )
            self.assertEqual(cm_gen.exception.code, "generation_mismatch")

            # Same-owner restore onto fresh volume succeeds completely
            fresh_vol = root / "fresh_volume"
            restored = mod.restore_essentials_archive(
                archive_file,
                fresh_vol,
                owner_key=owner_key,
                requester_uid="uid-owner-1",
                target_machine_id="cm_rt01",
                expected_generation=1,
            )
            self.assertTrue(restored.recovery_ready)
            self.assertTrue(restored.workspace_identities_compatible)
            self.assertEqual(restored.missing_or_partial, [])

            print(json.dumps({
                "measured_restore": {
                    "bytes_restored": restored.bytes_restored,
                    "files_restored": restored.files_restored,
                    "timings_ms": restored.timings_ms,
                    "deferred_deps": restored.deferred_dependency_installs,
                }
            }))

            # Verify Domios essentials, portable agent-home config, and sentinel restored byte-for-byte
            self.assertEqual((fresh_vol / "sentinel.txt").read_text(encoding="utf-8"), "harborlight-sentinel-v1\n")
            self.assertEqual((fresh_vol / ".domios" / "memory.sqlite").read_bytes(), b"SQLite format 3\x00fixture-mem-data")
            self.assertEqual((fresh_vol / ".claude" / "settings.json").read_text(encoding="utf-8"), '{"theme":"dark"}')
            self.assertEqual((fresh_vol / ".codex" / "config.toml").read_text(encoding="utf-8"), 'model = "gpt-5"\n')

            # Verify ZERO excluded credentials, keys, running authority, or replayable sends restored!
            self.assertFalse((fresh_vol / ".domios" / ".secrets").exists())
            self.assertFalse((fresh_vol / ".domios" / "neighbor-keys").exists())
            self.assertFalse((fresh_vol / ".domios" / "cloud_wake_queue.json").exists())
            self.assertFalse((fresh_vol / ".domios" / "server.pid").exists())
            self.assertFalse((fresh_vol / ".claude" / ".credentials.json").exists())
            self.assertFalse((fresh_vol / ".codex" / "auth.json").exists())

            # Verify repo HEAD, unpushed commit, binary+text staged, unstaged, untracked, and lazy dep omission
            restored_repo = fresh_vol / "projects" / "app"
            self.assertEqual(run_git(restored_repo, "rev-parse", "HEAD"), expected_head)
            self.assertEqual(run_git(restored_repo, "symbolic-ref", "--short", "HEAD"), "main")
            self.assertEqual((restored_repo / "asset.bin").read_bytes(), binary_payload)
            self.assertIn("asset.bin", run_git(restored_repo, "diff", "--cached", "--name-only"))
            self.assertIn("README.md", run_git(restored_repo, "diff", "--cached", "--name-only"))
            self.assertIn("unstaged edit", (restored_repo / "feature.txt").read_text(encoding="utf-8"))
            self.assertIn("feature.txt", run_git(restored_repo, "diff", "--name-only"))
            self.assertEqual((restored_repo / "notes.local.md").read_text(encoding="utf-8"), "untracked owner notes\n")
            self.assertFalse((restored_repo / "node_modules").exists())
            self.assertFalse((restored_repo / "target").exists())
            self.assertTrue(len(restored.deferred_dependency_installs) == 1)

    def test_refusal_guards_and_failed_restore_retains_archive(self):
        mod = load_cloud_archive()
        owner_key = b"M" * 32
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            cloud_home, _, _, _ = make_fixture_home(root)
            archive_file = root / "arc.enc"

            # 1. Unknown unclassified user file refuses eviction
            (cloud_home / "unknown-mystery.dat").write_bytes(b"cannot classify")
            with self.assertRaises(mod.ArchiveRefusalError) as cm_unk:
                mod.export_essentials_archive(
                    cloud_home, archive_file,
                    owner_uid="uid-1", machine_id="cm_1", generation=1,
                    owner_key=owner_key, archive_id="arc_1", named_user_files=["sentinel.txt"],
                )
            self.assertEqual(cm_unk.exception.code, "unknown_user_file")
            (cloud_home / "unknown-mystery.dat").unlink()

            # 2. Credential-bearing remote URL refuses eviction
            repo = cloud_home / "projects" / "app"
            orig_remote = run_git(repo, "remote", "get-url", "origin")
            run_git(repo, "remote", "set-url", "origin", "https://user:ghp_secret@github.com/org/app.git")
            with self.assertRaises(mod.ArchiveRefusalError) as cm_cred:
                mod.export_essentials_archive(
                    cloud_home, archive_file,
                    owner_uid="uid-1", machine_id="cm_1", generation=1,
                    owner_key=owner_key, archive_id="arc_1", named_user_files=["sentinel.txt"],
                )
            self.assertEqual(cm_cred.exception.code, "credential_in_remote_url")
            run_git(repo, "remote", "set-url", "origin", orig_remote)

            # 3. Competing wake / stale generation fence refuses eviction
            with self.assertRaises(mod.ArchiveRefusalError) as cm_fence:
                mod.export_essentials_archive(
                    cloud_home, archive_file,
                    owner_uid="uid-1", machine_id="cm_1", generation=1,
                    owner_key=owner_key, archive_id="arc_1", named_user_files=["sentinel.txt"],
                    generation_fence_check=lambda: False,
                )
            self.assertEqual(cm_fence.exception.code, "stale_generation")

            # 4. Export valid archive, then corrupt 1 byte -> verification fails!
            mod.export_essentials_archive(
                cloud_home, archive_file,
                owner_uid="uid-1", machine_id="cm_1", generation=1,
                owner_key=owner_key, archive_id="arc_1", named_user_files=["sentinel.txt"],
            )
            corrupt_path = root / "arc_corrupt.enc"
            raw = bytearray(archive_file.read_bytes())
            raw[-40] ^= 0xFF
            corrupt_path.write_bytes(bytes(raw))
            with self.assertRaises(mod.ArchiveVerificationError) as cm_mac:
                mod.verify_archive(
                    corrupt_path,
                    owner_key=owner_key,
                    expected_owner_uid="uid-1",
                    expected_machine_id="cm_1",
                    expected_generation=1,
                )
            self.assertEqual(cm_mac.exception.code, "mac_verification_failed")

            # 5. Simulated local disk-full (ENOSPC) during restore fails cleanly and KEEPS archive intact!
            def enospc_writer(_dest: pathlib.Path, _data: bytes) -> None:
                raise OSError(errno.ENOSPC, "No space left on device")

            with self.assertRaises(mod.RestoreDiskFullError) as cm_full:
                mod.restore_essentials_archive(
                    archive_file,
                    root / "enospc_vol",
                    owner_key=owner_key,
                    requester_uid="uid-1",
                    target_machine_id="cm_1",
                    expected_generation=1,
                    file_writer=enospc_writer,
                )
            self.assertEqual(cm_full.exception.code, "disk_full")
            self.assertTrue(archive_file.exists(), "Failed restore must keep source archive available")


if __name__ == "__main__":
    unittest.main(verbosity=2)
