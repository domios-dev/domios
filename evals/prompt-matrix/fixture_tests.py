"""Synthetic source controls; no provider, installer or network execution."""

import base64
from copy import deepcopy
import hashlib
import io
import json
from pathlib import Path
import tarfile
import tempfile
import time
import unittest

from coordinator_receipts import Phases, at, version_receipt
from coordinator_scenarios import preflight
from fixtures import HERE
from launch_env import isolated_environment, select_scope
from native_archive import extract, member_path, verify_archive, within
from native_assembly import load_native, qualify_receipt
from native_layout import FixtureError, recipe
from transport import Finding, Pending


class NativeControls(unittest.TestCase):
    def setUp(self):
        self.pins = json.loads((HERE / "versions.json").read_text())

    def select(self, provider="codex", platform="darwin", arch="arm64", libc="none", baseline=False):
        return recipe(self.pins, provider, self.pins["providers"][provider]["older"], platform, arch, libc, baseline)

    def test_claude_native_root_and_wrapper_destination(self):
        value = self.select("claude")
        self.assertEqual(value["native_executable"], "claude")
        self.assertEqual(value["assembled_executable"], "bin/claude.exe")

    def test_codex_vendor_tree_is_preserved(self):
        value = self.select()
        self.assertEqual(value["native_executable"], "vendor/aarch64-apple-darwin/bin/codex")

    def test_codex_linux_uses_musl_triple_on_glibc(self):
        self.assertEqual(self.select(platform="linux", arch="x64", libc="glibc")["native_executable"],
                         "vendor/x86_64-unknown-linux-musl/bin/codex")

    def test_opencode_explicit_baseline_musl(self):
        self.assertEqual(self.select("opencode", "linux", "x64", "musl", True)["native_package"],
                         "opencode-linux-x64-baseline-musl")

    def test_windows_native_suffix(self):
        self.assertEqual(self.select("claude", "win32", "x64")["native_executable"], "claude.exe")

    def test_unknown_platform_is_red(self):
        with self.assertRaisesRegex(FixtureError, "Unsupported platform"):
            self.select(platform="android")

    def test_unknown_architecture_is_red(self):
        with self.assertRaisesRegex(FixtureError, "Unsupported platform"):
            self.select(arch="riscv64")

    def test_implicit_linux_libc_is_red(self):
        with self.assertRaisesRegex(FixtureError, "libc"):
            self.select(platform="linux")

    def test_wrong_literal_is_red(self):
        with self.assertRaisesRegex(FixtureError, "reviewed literal"):
            recipe(self.pins, "codex", "0.0.1", "darwin", "arm64")

    def test_missing_optional_dependency_is_red(self):
        self.pins["providers"]["codex"]["versions"]["0.159.3"]["optional_dependencies"].clear()
        with self.assertRaisesRegex(FixtureError, "dependency absent"):
            self.select()

    def test_wrong_native_alias_version_is_red(self):
        self.pins["providers"]["codex"]["versions"]["0.159.3"]["native_distribution_metadata"][
            "@openai/codex-darwin-arm64"]["version"] = "other"
        with self.assertRaisesRegex(FixtureError, "alias version"):
            self.select()

    def test_arm_baseline_is_red(self):
        with self.assertRaisesRegex(FixtureError, "Baseline"):
            self.select("opencode", baseline=True)

    def test_archive_traversal_is_red(self):
        with self.assertRaisesRegex(FixtureError, "traversal"):
            member_path("package/../outside")

    def test_archive_absolute_path_is_red(self):
        with self.assertRaisesRegex(FixtureError, "Unsafe archive path"):
            member_path("/package/absolute")

    def test_archive_windows_path_is_red(self):
        with self.assertRaisesRegex(FixtureError, "Unsafe archive path"):
            member_path("package/C:\\escape")

    def test_reserved_windows_component_is_red(self):
        with self.assertRaisesRegex(FixtureError, "component"):
            member_path("package/con.txt")

    def test_matching_digest_is_a_control(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "opaque.tgz"
            path.write_bytes(b"opaque test bytes")
            metadata = {"integrity": "sha512-" + base64.b64encode(hashlib.sha512(path.read_bytes()).digest()).decode(),
                        "tarball_sha256": hashlib.sha256(path.read_bytes()).hexdigest(), "tarball": "opaque-source"}
            self.assertEqual(verify_archive(path, metadata)["sha256"], metadata["tarball_sha256"])

    def test_wrong_sha512_is_red(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "opaque.tgz"
            path.write_bytes(b"opaque")
            with self.assertRaisesRegex(FixtureError, "SHA512"):
                verify_archive(path, {"integrity": "sha512-wrong"})

    def test_wrong_sha256_is_red(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "opaque.tgz"
            path.write_bytes(b"opaque")
            metadata = {"integrity": "sha512-" + base64.b64encode(hashlib.sha512(b"opaque").digest()).decode(),
                        "tarball_sha256": "wrong"}
            with self.assertRaisesRegex(FixtureError, "SHA256"):
                verify_archive(path, metadata)

    def archive(self, entries):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name).resolve()
        archive = root / "archive.tgz"
        with tarfile.open(archive, "w:gz") as output:
            for name, kind, data in entries:
                member = tarfile.TarInfo(name)
                member.type = kind
                if kind == tarfile.REGTYPE:
                    member.size = len(data)
                    output.addfile(member, io.BytesIO(data))
                else:
                    member.linkname = data
                    output.addfile(member)
        return archive, root / "out"

    def test_safe_regular_extraction_control(self):
        archive, destination = self.archive([("package/bin/opaque", tarfile.REGTYPE, b"opaque")])
        receipt = extract(archive, destination, time.monotonic() + 5)
        self.assertEqual((destination / "bin/opaque").read_bytes(), b"opaque")
        self.assertEqual(len(receipt["files"]), 1)

    def test_archive_symlink_is_red(self):
        archive, destination = self.archive([("package/link", tarfile.SYMTYPE, "../../outside")])
        with self.assertRaisesRegex(FixtureError, "links"):
            extract(archive, destination, time.monotonic() + 5)

    def test_archive_hardlink_is_red(self):
        archive, destination = self.archive([("package/link", tarfile.LNKTYPE, "package/file")])
        with self.assertRaisesRegex(FixtureError, "links"):
            extract(archive, destination, time.monotonic() + 5)

    def test_archive_case_collision_is_red(self):
        archive, destination = self.archive([("package/A", tarfile.REGTYPE, b"a"),
                                             ("package/a", tarfile.REGTYPE, b"b")])
        with self.assertRaisesRegex(FixtureError, "case-colliding"):
            extract(archive, destination, time.monotonic() + 5)

    def test_absent_native_is_red(self):
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(FixtureError, "manifest is absent"):
                load_native(Path(temporary).resolve(), "codex", "0.159.3")

    def qualification(self):
        from native_version_tests import qualification_fixture
        return qualification_fixture()

    def reject_qualification(self, change):
        receipt = self.qualification()
        request = receipt["request"]
        version, executable, executable_hash = request["version"], request["executable"], request["executable_sha256"]
        change(receipt)
        with self.assertRaisesRegex(FixtureError, "unqualified"):
            qualify_receipt(receipt, version, executable, executable_hash)

    def test_qualified_receipt_control(self):
        receipt = self.qualification()
        qualify_receipt(receipt, receipt["version"], receipt["argv"][0], receipt["executable_sha256"])

    def test_not_run_native_receipt_is_red(self):
        self.reject_qualification(lambda value: value.update(status="NOT RUN"))

    def test_wrong_native_version_receipt_is_red(self):
        self.reject_qualification(lambda value: value.update(version="other"))

    def test_changed_executable_receipt_is_red(self):
        self.reject_qualification(lambda value: value.update(executable_sha256="other"))

    def test_owner_executable_receipt_is_red(self):
        self.reject_qualification(lambda value: value.update(argv=["owner-executable", "--version"]))

    def test_nonzero_native_version_receipt_is_red(self):
        self.reject_qualification(lambda value: value.update(exit_code=1))


class IsolationControls(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.case = self.root / "case"
        self.case.mkdir()
        self.home = self.case / "home"
        self.home.mkdir()
        self.cwd = self.case / "cwd"
        self.cwd.mkdir()
        self.row = {"scope_id": "supplied-scope", "provider": "codex", "case_root": str(self.case),
                    "home": str(self.home), "cwd": str(self.cwd)}

    def test_owner_credentials_are_removed(self):
        parent = {"OPENAI_API_KEY": "opaque", "ANTHROPIC_AUTH_TOKEN": "opaque", "NODE_OPTIONS": "opaque",
                  "BASH_ENV": "opaque", "HTTP_PROXY": "opaque", "HOME": "/owner", "CODEX_HOME": "/owner"}
        env = isolated_environment(parent, self.root, self.home)
        self.assertFalse(set(parent) - {"HOME", "CODEX_HOME"} & set(env))
        self.assertEqual(env["HOME"], str(self.home))
        self.assertEqual(env["CODEX_HOME"], str(self.home / ".codex"))

    def test_transport_identity_is_retained(self):
        parent = {"CODETERM_TAB_ID": "supplied-tab", "CODETERM_API_TOKEN": "opaque",
                  "CODETERM_SESSION_BINDING_NONCE": "supplied-nonce", "CODETERM_STARTUP_ATTEMPT_ID": "supplied-launch"}
        env = isolated_environment(parent, self.root, self.home)
        self.assertTrue(all(env[key] == value for key, value in parent.items()))

    def test_owner_home_is_red(self):
        with self.assertRaisesRegex(FixtureError, "escaped fixture"):
            isolated_environment({}, self.root, self.root.parent / "owner")

    def test_exact_case_scope_is_control(self):
        self.assertEqual(select_scope(self.root, [self.row], "codex", self.cwd)["scope_id"], "supplied-scope")

    def test_cross_case_cwd_is_red(self):
        with self.assertRaisesRegex(FixtureError, "unique exact"):
            select_scope(self.root, [self.row], "codex", self.case)

    def test_duplicate_scope_is_red(self):
        with self.assertRaisesRegex(FixtureError, "unique exact"):
            select_scope(self.root, [self.row, self.row], "codex", self.cwd)

    def test_cross_provider_scope_is_red(self):
        with self.assertRaisesRegex(FixtureError, "unique exact"):
            select_scope(self.root, [self.row], "claude", self.cwd)

    def test_escaping_home_is_red(self):
        self.row["home"] = str(self.root / "other-case")
        with self.assertRaisesRegex(FixtureError, "escaped fixture"):
            select_scope(self.root, [self.row], "codex", self.cwd)

    def test_symlink_home_is_red(self):
        from unittest.mock import patch
        with patch.object(Path, "is_symlink", return_value=True):
            with self.assertRaisesRegex(FixtureError, "Symlink"):
                within(self.root, self.home)



class CoordinatorPhaseControls(unittest.TestCase):
    def test_default_order_control(self):
        phases = Phases("default")
        for sequence, phase in enumerate(("prompt", "installed", "resumed", "verified"), 1):
            phases.advance(phase, sequence)
        self.assertEqual(phases.current, "verified")

    def test_busy_order_control(self):
        phases = Phases("busy")
        for sequence, phase in enumerate(("busy_started", "prompt", "installed_busy", "released", "idle", "resumed", "verified"), 1):
            phases.advance(phase, sequence)
        self.assertTrue(phases.released)

    def test_failure_requires_tree_and_barrier(self):
        phases = Phases("fail")
        for sequence, phase in enumerate(("prompt", "failed", "tree_stopped", "failure_barrier", "verified"), 1):
            phases.advance(phase, sequence)
        self.assertEqual(phases.current, "verified")

    def test_install_before_prompt_is_red(self):
        with self.assertRaisesRegex(Finding, "out of order"):
            Phases("default").advance("installed", 1)

    def test_replayed_event_is_red(self):
        phases = Phases("default")
        phases.advance("prompt", 2)
        with self.assertRaisesRegex(Finding, "stale"):
            phases.advance("installed", 2)

    def test_parent_exit_cannot_replace_tree_receipt(self):
        phases = Phases("fail")
        phases.advance("prompt", 1)
        phases.advance("failed", 2)
        with self.assertRaisesRegex(Finding, "out of order"):
            phases.advance("failure_barrier", 3)

    def test_missing_actual_receipt_stays_pending(self):
        with self.assertRaisesRegex(Pending, "absent"):
            at({"claimed": True}, None)

    def test_current_contract_preflight_stays_pending(self):
        contracts = {"isolation": {"status": "pending", "reason": "supplied missing fixture auth"}}
        with self.assertRaisesRegex(Pending, "supplied missing fixture auth"):
            preflight(contracts, None, None)


class VersionToolControls(unittest.TestCase):
    def fixture(self):
        call = {"id": "supplied-call-id", "toolName": "supplied-tool", "toolCallSourceId": "supplied-source",
                "toolId": "supplied-tool-id", "toolInput": {"command": "supplied-tool-command"}}
        result = {"id": "supplied-result-id", "toolCallSourceId": "supplied-source", "toolId": "supplied-tool-id",
                  "toolPairing": "exact", "toolExitCode": 0,
                  "toolResult": json.dumps({"nonce": "supplied-nonce", "argv": ["codex", "--version"],
                                            "exit_code": 0, "stdout": "codex 0.160.0"})}
        expected = {"tab": "own", "session": "supplied-session", "launch": "supplied-launch", "version": "0.160.0"}
        return [call, result], expected

    def reject(self, mutation):
        rows, expected = self.fixture()
        mutation(rows)
        with self.assertRaisesRegex(Finding, "exact own-session"):
            version_receipt(rows, expected, "supplied-tool-command", "supplied-nonce")

    def test_actual_source_fields_control(self):
        rows, expected = self.fixture()
        self.assertEqual(version_receipt(rows, expected, "supplied-tool-command", "supplied-nonce")["version"], "0.160.0")

    def test_global_absolute_binary_is_red(self):
        self.reject(lambda rows: rows[0]["toolInput"].update(command="/global/codex --version"))

    def test_missing_exit_code_is_red(self):
        self.reject(lambda rows: rows[1].pop("toolExitCode"))

    def test_unavailable_pairing_is_red(self):
        self.reject(lambda rows: rows[1].update(toolPairing="unavailable"))

    def test_duplicate_results_are_red(self):
        self.reject(lambda rows: rows.append(deepcopy(rows[1])))

    def test_wrong_nonce_is_red(self):
        self.reject(lambda rows: rows[1].update(toolResult=json.dumps({"nonce": "other", "argv": ["codex", "--version"], "exit_code": 0, "stdout": "codex 0.160.0"})))

    def test_wrong_target_is_red(self):
        self.reject(lambda rows: rows[1].update(toolResult=json.dumps({"nonce": "supplied-nonce", "argv": ["codex", "--version"], "exit_code": 0, "stdout": "codex 0.159.3"})))
