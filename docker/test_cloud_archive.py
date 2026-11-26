"""End-to-end fixture round-trip, streaming AES-256-GCM AEAD crypto, signed-URL HTTP upload/download,
refusal guards, and 200 MB streaming benchmark tests for Domios portable essentials archives.
"""

from __future__ import annotations

from dataclasses import asdict
import hashlib
import http.server
import importlib.util
import json
import os
import pathlib
import struct
import subprocess
import sys
import tempfile
import threading
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
        owner_key = os.urandom(32)
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
            )
            self.assertTrue(exported.excluded_secrets_verified)
            self.assertGreater(exported.archive_bytes, 0)
            self.assertTrue(archive_file.read_bytes().startswith(mod.WIRE_MAGIC))
            self.assertEqual(exported.repos[0].head_sha, expected_head)
            self.assertIsNotNone(exported.repos[0].bundle_member)
            self.assertIsNotNone(exported.repos[0].staged_patch_member)
            self.assertIsNotNone(exported.repos[0].unstaged_patch_member)
            self.assertIn("projects/app/node_modules", exported.omitted_regenerable_dirs)
            self.assertIn("projects/app/target", exported.omitted_regenerable_dirs)

            print(json.dumps({
                "measured_export": {
                    "archive_bytes": exported.archive_bytes,
                    "uncompressed_bytes": exported.uncompressed_bytes,
                    "timings_ms": asdict(exported.timings_ms),
                }
            }))

            # Cross-owner, wrong-machine, and wrong-generation restore MUST fail AEAD auth before touching target
            bad_target = root / "cross_owner_vol"
            with self.assertRaises(mod.ArchiveError) as cm_owner:
                mod.restore_essentials_archive(
                    archive_file,
                    bad_target,
                    owner_key=owner_key,
                    requester_uid="uid-other-2",
                    target_machine_id="cm_rt01",
                    expected_generation=1,
                )
            self.assertEqual(cm_owner.exception.code, "auth_failed")

            with self.assertRaises(mod.ArchiveError) as cm_gen:
                mod.restore_essentials_archive(
                    archive_file,
                    bad_target,
                    owner_key=owner_key,
                    requester_uid="uid-owner-1",
                    target_machine_id="cm_rt01",
                    expected_generation=99,
                )
            self.assertEqual(cm_gen.exception.code, "auth_failed")

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
            self.assertEqual(restored.missing_or_partial, [])

            print(json.dumps({
                "measured_restore": {
                    "bytes_restored": restored.bytes_restored,
                    "files_restored": restored.files_restored,
                    "timings_ms": asdict(restored.timings_ms),
                    "deferred_installs": restored.deferred_installs,
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
            self.assertEqual(len(restored.deferred_installs), 1)

    def test_streaming_aead_chunk_security_and_signed_http_upload_download(self):
        mod = load_cloud_archive()
        key = os.urandom(32)
        payload = os.urandom(20_000)  # spans 5 chunks at 4096 B chunk_size
        ct = mod.encrypt_archive_bytes(payload, key, "u1", "m1", 3, chunk_size=4096)
        self.assertEqual(mod.decrypt_archive_bytes(ct, key, "u1", "m1", 3), payload)

        # 1. Chunk truncation (drop final chunk) MUST fail
        hdr_len = mod.WIRE_HEADER_STRUCT.size
        first_ct_len, _ = mod.WIRE_CHUNK_STRUCT.unpack_from(ct, hdr_len)
        truncated = ct[: hdr_len + mod.WIRE_CHUNK_STRUCT.size + first_ct_len]
        with self.assertRaises(mod.ArchiveError) as cm_trunc:
            mod.decrypt_archive_bytes(truncated, key, "u1", "m1", 3)
        self.assertEqual(cm_trunc.exception.code, "corrupt_archive")

        # 2. Chunk reordering (swap chunk 0 and chunk 1) MUST fail AEAD tag (chunk index in AAD + nonce)
        off0 = hdr_len
        len0, flg0 = mod.WIRE_CHUNK_STRUCT.unpack_from(ct, off0)
        chunk0 = ct[off0 : off0 + mod.WIRE_CHUNK_STRUCT.size + len0]
        off1 = off0 + len(chunk0)
        len1, flg1 = mod.WIRE_CHUNK_STRUCT.unpack_from(ct, off1)
        chunk1 = ct[off1 : off1 + mod.WIRE_CHUNK_STRUCT.size + len1]
        swapped = ct[:hdr_len] + chunk1 + chunk0 + ct[off1 + len(chunk1) :]
        with self.assertRaises(mod.ArchiveError) as cm_swap:
            mod.decrypt_archive_bytes(swapped, key, "u1", "m1", 3)
        self.assertEqual(cm_swap.exception.code, "auth_failed")

        # 3. Real loopback HTTP signed-URL `export-and-upload` + `download_encrypted_archive`
        store: dict[str, bytes] = {}
        expected_token = "sig-one-object-short-lived"

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, format, *args):
                return

            def do_PUT(self):
                if f"token={expected_token}" not in self.path:
                    self.send_response(403)
                    self.end_headers()
                    return
                length = int(self.headers.get("Content-Length", "0"))
                store[self.path.split("?")[0]] = self.rfile.read(length)
                self.send_response(200)
                self.end_headers()

            def do_GET(self):
                if f"token={expected_token}" not in self.path:
                    self.send_response(403)
                    self.end_headers()
                    return
                data = store.get(self.path.split("?")[0])
                if data is None:
                    self.send_response(404)
                    self.end_headers()
                    return
                self.send_response(200)
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        port = server.server_address[1]
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            with tempfile.TemporaryDirectory() as tmp:
                root = pathlib.Path(tmp)
                cloud_home, _, expected_head, _ = make_fixture_home(root)
                upload_url = f"http://127.0.0.1:{port}/cloud-archives/u1/cm_http/gen-1/arc_http.enc?token={expected_token}"
                env = dict(os.environ)
                env.pop("DOMIOS_EXPORT_ARCHIVE_KEY_HEX", None)
                proc = subprocess.run(
                    [sys.executable, str(REPO / "packaging/docker/cloud_archive.py"), "export-and-upload"],
                    input=json.dumps({
                        "home_dir": str(cloud_home),
                        "archive_id": "arc_http",
                        "owner_uid": "u1",
                        "machine_id": "cm_http",
                        "generation": 1,
                        "upload_url": upload_url,
                        "owner_key_hex": key.hex(),
                    }),
                    text=True,
                    capture_output=True,
                    env=env,
                    check=True,
                )
                receipt = json.loads(proc.stdout)
                self.assertTrue(receipt["excluded_secrets_verified"])
                self.assertEqual(receipt["upload"]["status"], 200)
                uploaded_bytes = store["/cloud-archives/u1/cm_http/gen-1/arc_http.enc"]
                self.assertEqual(hashlib.sha256(uploaded_bytes).hexdigest(), receipt["ciphertext_sha256"])

                # Download via signed GET URL and restore cleanly
                dl_path = root / "downloaded.enc"
                dl_size, dl_sha = mod.download_encrypted_archive(
                    upload_url, dl_path, expected_sha256=receipt["ciphertext_sha256"]
                )
                self.assertEqual(dl_size, receipt["archive_bytes"])
                self.assertEqual(dl_sha, receipt["ciphertext_sha256"])

                restored_dir = root / "restored_http"
                restored = mod.restore_essentials_archive(
                    dl_path,
                    restored_dir,
                    owner_key=key,
                    requester_uid="u1",
                    target_machine_id="cm_http",
                    expected_generation=1,
                )
                self.assertTrue(restored.recovery_ready)
                self.assertEqual(run_git(restored_dir / "projects" / "app", "rev-parse", "HEAD"), expected_head)
        finally:
            server.shutdown()
            server.server_close()

    def test_refusal_guards_and_failed_restore_retains_archive(self):
        mod = load_cloud_archive()
        owner_key = os.urandom(32)
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            cloud_home, _, _, _ = make_fixture_home(root)
            archive_file = root / "arc.enc"

            # 1. Credential-bearing remote URL refuses eviction
            repo = cloud_home / "projects" / "app"
            orig_remote = run_git(repo, "remote", "get-url", "origin")
            run_git(repo, "remote", "set-url", "origin", "https://user:ghp_secret@github.com/org/app.git")
            with self.assertRaises(mod.ArchiveError) as cm_cred:
                mod.export_essentials_archive(
                    cloud_home, archive_file,
                    owner_uid="uid-1", machine_id="cm_1", generation=1,
                    owner_key=owner_key, archive_id="arc_1",
                )
            self.assertEqual(cm_cred.exception.code, "repo_remote_has_credentials")
            run_git(repo, "remote", "set-url", "origin", orig_remote)

            # 2. Competing wake / stale generation fence refuses eviction
            with self.assertRaises(mod.ArchiveError) as cm_fence:
                mod.export_essentials_archive(
                    cloud_home, archive_file,
                    owner_uid="uid-1", machine_id="cm_1", generation=1,
                    owner_key=owner_key, archive_id="arc_1",
                    generation_fence_check=lambda: False,
                )
            self.assertEqual(cm_fence.exception.code, "stale_generation")

            # 3. Export valid archive, then corrupt 1 byte -> verification fails!
            mod.export_essentials_archive(
                cloud_home, archive_file,
                owner_uid="uid-1", machine_id="cm_1", generation=1,
                owner_key=owner_key, archive_id="arc_1",
            )
            corrupt_path = root / "arc_corrupt.enc"
            raw = bytearray(archive_file.read_bytes())
            raw[-20] ^= 0xFF
            corrupt_path.write_bytes(bytes(raw))
            with self.assertRaises(mod.ArchiveError) as cm_mac:
                mod.verify_archive(
                    corrupt_path,
                    owner_key=owner_key,
                    expected_owner_uid="uid-1",
                    expected_machine_id="cm_1",
                    expected_generation=1,
                )
            self.assertEqual(cm_mac.exception.code, "auth_failed")

            # 4. Simulated local disk-full (ENOSPC) during restore fails cleanly and KEEPS archive intact!
            with self.assertRaises(mod.RestoreDiskFullError) as cm_full:
                mod.restore_essentials_archive(
                    archive_file,
                    root / "enospc_vol",
                    owner_key=owner_key,
                    requester_uid="uid-1",
                    target_machine_id="cm_1",
                    expected_generation=1,
                    simulated_disk_full_after_bytes=64,
                )
            self.assertEqual(cm_full.exception.code, "disk_full_local")
            self.assertTrue(archive_file.exists(), "Failed restore must keep source archive available")


if __name__ == "__main__":
    unittest.main(verbosity=2)
