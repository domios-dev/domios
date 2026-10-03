"""WHAT: independent canonical-map and bounded-response regression oracles.
WHY: the legacy loader rejects shipped bytes and the network reader is unbounded.
ORACLE: exact supplied map round-trip; small response succeeds, oversize fails.
Synthetic mechanism fixtures only; no event, credential or live-data evidence.
"""

from contextlib import ExitStack
import hashlib
import json
from pathlib import Path, PurePosixPath, PureWindowsPath
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

import jev_transport as transport_module
import jev_request_worker as worker_module
import local_jev_transport_io as local_module

import run
import request_contract as contract
from jev_request_worker import read_bounded_response, WorkerError
from jev_transport import TransportFailure, TransportResult


class SyntheticReadTransport:
    def __init__(self, response):
        self.response = response

    def send(self, raw, **kwargs):
        try:
            body = read_bounded_response(self.response, end=60, clock=lambda: 0)
            return TransportResult(contract.decode(body, contract.MAX_RESPONSE),
                                   {"fixture_type": "synthetic_mechanism"})
        except WorkerError as error:
            raise TransportFailure(error.code) from error


class BoundedResponse:
    def __init__(self, raw):
        self.raw = raw
        self.offset = 0
        self.bounds = []

    def __enter__(self):
        return self

    def __exit__(self, *args):
        return False

    def read(self, size=None):
        if not isinstance(size, int) or size <= 0:
            raise AssertionError("response read requires an explicit positive byte bound")
        self.bounds.append(size)
        chunk = self.raw[self.offset:self.offset + size]
        self.offset += len(chunk)
        return chunk


def valid_response():
    return json.dumps({"answers": {"q": {
        "type": "choice", "choice": "ready_idle",
        "probabilities": {"ready_idle": 0.95},
    }}}).encode()


class LegacyRegressionTests(unittest.TestCase):
    def test_canonical_criteria_plain_map_is_loadable(self):
        path = Path(run.__file__).with_name("criteria.json")
        supplied = path.read_bytes()
        self.assertEqual(run.load_criteria(path, "current"), json.loads(supplied))

    def test_response_reads_are_explicitly_byte_bounded(self):
        small = BoundedResponse(valid_response())
        oversized = BoundedResponse(valid_response() + b" " * (64 * 1024 + 1))
        for response, expected_error in ((small, None), (oversized, "response_oversize")):
            with self.subTest(oversized=expected_error is not None):
                answer, _, error = run.call_jev(
                    "synthetic mechanism screen", {}, arm="screen",
                    criteria={label: "opaque" for label in run.LABELS},
                    token="synthetic-mechanism-token", endpoint=run.DEFAULT_ENDPOINT,
                    model="synthetic-alias", timeout=60, transport=SyntheticReadTransport(response),
                )
                self.assertEqual(error, expected_error)
                self.assertTrue(response.bounds)
                if expected_error is None:
                    self.assertEqual(answer["prediction"], "ready_idle")
                else:
                    self.assertIsNone(answer)


class WindowsPlatformTests(unittest.TestCase):
    def test_captured_windows_process_handle_is_closed_exactly_once(self):
        class CapturedHandle:
            def __int__(self):
                return 77
        handle = CapturedHandle()
        handle.Close = Mock()
        kernel = Mock()
        kernel.WaitForSingleObject.return_value = 0
        def exit_code(captured, pointer):
            pointer._obj.value = 0
            return 1
        kernel.GetExitCodeProcess.side_effect = exit_code
        with patch.object(transport_module.ctypes, "WinDLL", return_value=kernel, create=True):
            authority = transport_module.ProcessAuthority(
                SimpleNamespace(pid=999, returncode=None, _handle=handle), windows=True)
            self.assertEqual(authority.wait_exact(), 0)
            authority.close()
            authority.close()
            self.assertFalse(authority.cancel())
        handle.Close.assert_called_once()
        kernel.WaitForSingleObject.assert_called_once_with(77, 0xFFFFFFFF)
        kernel.TerminateProcess.assert_not_called()


class WindowsApiFixture:
    def __init__(self):
        self.handles, self.closed, self.created, self.removed = {}, [], [], []
        self.override = None
        self.close_error = None
        self.raw = b"opaque source bytes"

    def current_user(self):
        return "S-1-5-21-42"

    def open(self, path, *, directory=False):
        handle = len(self.handles) + 1
        self.handles[handle] = {"path": str(path), "directory": directory, "offset": 0}
        return handle

    def inspect(self, handle):
        entry = self.handles[handle]
        security = {"owner": self.current_user(), "dacl_present": True, "protected": True,
                    "aces": [(0, 0, 0x1f01ff, self.current_user())], "bytes": b"opaque descriptor"}
        facts = {"identity": entry["path"].encode(), "directory": entry["directory"],
                 "size": 0 if entry["directory"] else len(self.raw), "security": security,
                 "signature": (entry["path"], b"opaque descriptor"), "final": entry["path"]}
        if self.override:
            self.override(facts)
        return facts

    def read(self, handle):
        entry = self.handles[handle]
        offset = entry["offset"]
        entry["offset"] += 4096
        return self.raw[offset:offset + 4096]

    def create_directory(self, path, sddl):
        self.created.append((str(path), sddl))

    def remove_directory(self, path):
        self.removed.append(str(path))
        return True

    def close(self, handle):
        self.closed.append(handle)
        if self.close_error:
            raise self.close_error


class WindowsTrustTests(unittest.TestCase):
    def test_servicing_root_is_admitted_through_captured_file_and_directory_ancestors(self):
        for directory in (False, True):
            with self.subTest(directory=directory), ExitStack() as stack:
                guard, api = self.guard(stack)
                def servicing_root(facts):
                    if facts["final"] == "C:\\":
                        facts["security"]["owner"] = worker_module.WINDOWS_SERVICING_OWNER
                        facts["security"]["aces"] += [(0, 0, 4, "S-1-5-11"), (0, 8, 0x1f01ff, "S-1-5-11")]
                api.override = servicing_root
                if directory:
                    guard.directory(r"C:\fixture")
                else:
                    guard.file(r"C:\fixture\contract.py")
                self.assertEqual(guard.objects[0]["path"], PureWindowsPath("C:\\"))
                self.assertEqual(guard.objects[0]["facts"]["security"]["owner"], worker_module.WINDOWS_SERVICING_OWNER)
                self.assertEqual(worker_module.WINDOWS_ADMINS, frozenset(("S-1-5-18", "S-1-5-32-544")))
                guard.revalidate()
                guard.close()
                self.assertTrue(guard.closed)
                self.assertEqual(len(api.closed), len(api.handles))
                self.assertEqual(len(set(api.closed)), len(api.closed))

    def test_servicing_owner_refuses_nonroot_leaf_private_and_other_service_contexts(self):
        cases = ((r"C:\fixture", True, True, False, worker_module.WINDOWS_SERVICING_OWNER),
                 (r"C:\fixture\leaf.py", False, False, False, worker_module.WINDOWS_SERVICING_OWNER),
                 ("C:\\", True, False, False, worker_module.WINDOWS_SERVICING_OWNER),
                 ("C:\\", True, True, True, worker_module.WINDOWS_SERVICING_OWNER),
                 ("C:\\", True, True, False, "S-1-5-80-123"))
        for path, directory, ancestor, private, sid in cases:
            with self.subTest(path=path, ancestor=ancestor, private=private, sid=sid), ExitStack() as stack:
                guard, api = self.guard(stack)
                api.override = lambda facts: facts["security"].update(owner=sid)
                with self.assertRaisesRegex(WorkerError, "windows_owner"):
                    guard.opened(path, directory=directory, ancestor=ancestor, private=private)
                guard.close()
                self.assertEqual(api.closed, [1])
        for ancestor, private in ((False, False), (True, True)):
            with self.subTest(ancestor=ancestor, private=private), self.assertRaisesRegex(WorkerError, "windows_owner"):
                worker_module.windows_acl_policy(self.security(owner=worker_module.WINDOWS_SERVICING_OWNER),
                                                 "S-1-5-21-42", volume_root=True, ancestor=ancestor, private=private)

    def test_root_identity_is_confirmed_before_servicing_owner_policy(self):
        for updates in ({"final": r"C:\replacement"}, {"directory": False}):
            with self.subTest(updates=updates), ExitStack() as stack:
                guard, api = self.guard(stack)
                def mismatched(facts):
                    facts.update(updates)
                    facts["security"]["owner"] = worker_module.WINDOWS_SERVICING_OWNER
                api.override = mismatched
                policy = stack.enter_context(patch.object(worker_module, "windows_acl_policy", wraps=worker_module.windows_acl_policy))
                with self.assertRaisesRegex(WorkerError, "windows_path_identity"):
                    guard.opened("C:\\", directory=True, ancestor=True)
                policy.assert_not_called()
                guard.close()
                self.assertEqual(api.closed, [1])

    def test_servicing_root_owner_does_not_exempt_dacl_or_service_ace_checks(self):
        cases = [({"dacl_present": False}, "windows_dacl"), ({"aces": None}, "windows_dacl"),
                 ({"aces": [(5, 0, 1, "S-1-1-0")]}, "windows_ace_unsupported"),
                 ({"aces": [(0, 0x20, 1, "S-1-1-0")]}, "windows_ace_unsupported")]
        for sid in ("S-1-1-0", worker_module.WINDOWS_SERVICING_OWNER):
            for mask in (0x40, 0x10000, 0x40000, 0x80000, 0x10000000, 0x40000000):
                cases.append(({"aces": [(0, 0, mask, sid)]}, "windows_foreign_authority"))
        for updates, code in cases:
            with self.subTest(updates=updates), ExitStack() as stack:
                guard, api = self.guard(stack)
                api.override = lambda facts: facts["security"].update(owner=worker_module.WINDOWS_SERVICING_OWNER, **updates)
                with self.assertRaisesRegex(WorkerError, code):
                    guard.opened("C:\\", directory=True, ancestor=True)
                guard.close()
                self.assertEqual(api.closed, [1])
        with ExitStack() as stack:
            guard, api = self.guard(stack)
            api.inspect = Mock(side_effect=WorkerError("windows_security_read"))
            with self.assertRaisesRegex(WorkerError, "windows_security_read"):
                guard.opened("C:\\", directory=True, ancestor=True)
            guard.close()
            self.assertEqual(api.closed, [1])

    def runtime_with_digest(self, stack, digest):
        guard, _ = self.guard(stack)
        stack.enter_context(patch.object(worker_module.sys, "base_prefix", r"C:\fixture"))
        stack.enter_context(patch.object(worker_module.sys, "prefix", r"C:\fixture"))
        stack.enter_context(patch.object(worker_module.sys, "platform", "win32"))
        stack.enter_context(patch.object(worker_module.sys, "version_info", (3, 14, 6)))
        stack.enter_context(patch.object(worker_module.sys, "implementation", SimpleNamespace(name="cpython")))
        stack.enter_context(patch.object(worker_module.sysconfig, "get_platform", return_value="win-amd64"))
        stack.enter_context(patch.object(worker_module.sysconfig, "get_config_var", return_value=0))
        stack.enter_context(patch.object(PureWindowsPath, "exists", return_value=False, create=True))
        stack.enter_context(patch.object(PureWindowsPath, "absolute", lambda path: path, create=True))
        guard.directory = Mock()
        guard.file = Mock(return_value=(None, digest, None))
        return guard

    def test_windows_distribution_source_digest_is_admitted_exactly(self):
        with ExitStack() as stack:
            guard = self.runtime_with_digest(
                stack, "a7b6bdf4657b018e4e76f22ebbb975a5fa6f0501088177fe950df1e20c18b209")
            guard.runtime(r"C:\fixture\python.exe")
            self.assertEqual(guard.file.call_args.args[0], PureWindowsPath(r"C:\fixture\Lib\subprocess.py"))
            guard.close()

    def test_other_source_digests_and_open_errors_refuse_runtime_admission(self):
        for digest in ("6628ffdd65c093a6c08cae01ffe82877d3ced515aac9e7be0cff16512c30a7d9", "0" * 64):
            with self.subTest(digest=digest), ExitStack() as stack:
                guard = self.runtime_with_digest(stack, digest)
                with self.assertRaisesRegex(WorkerError, "windows_runtime_source"):
                    guard.runtime(r"C:\fixture\python.exe")
                guard.close()
        with ExitStack() as stack:
            guard = self.runtime_with_digest(stack, None)
            guard.file.side_effect = WorkerError("windows_file_identity")
            with self.assertRaisesRegex(WorkerError, "windows_file_identity"):
                guard.runtime(r"C:\fixture\python.exe")
            guard.close()

    def security(self, **updates):
        return {"owner": "S-1-5-21-42", "dacl_present": True, "protected": True,
                "aces": [(0, 0, 0x1f01ff, "S-1-5-21-42")], "bytes": b"supplied descriptor", **updates}

    def guard(self, stack, api=None, clock=None):
        stack.enter_context(patch.object(worker_module, "Path", PureWindowsPath))
        stack.enter_context(patch.object(worker_module.os.path, "abspath", side_effect=lambda path: str(path)))
        api = WindowsApiFixture() if api is None else api
        return worker_module.WindowsTrust(55, api=api, clock=clock or (lambda: 0)), api

    def test_current_token_system_and_administration_principals_are_explicit(self):
        supplied = self.security(aces=[(0, 0, 0x1f01ff, sid) for sid in (
            "S-1-5-21-42", "S-1-5-18", "S-1-5-32-544")])
        worker_module.windows_acl_policy(supplied, "S-1-5-21-42", private=True)
        for sid in ("S-1-1-0", "S-1-5-32-545", "S-1-5-21-99"):
            with self.assertRaisesRegex(WorkerError, "windows_owner"):
                worker_module.windows_acl_policy(self.security(owner=sid), "S-1-5-21-42")

    def test_absent_null_and_unreadable_dacls_refuse(self):
        for security in (self.security(dacl_present=False), self.security(aces=None)):
            with self.assertRaisesRegex(WorkerError, "windows_dacl"):
                worker_module.windows_acl_policy(security, "S-1-5-21-42")
        with ExitStack() as stack:
            guard, api = self.guard(stack)
            api.inspect = Mock(side_effect=WorkerError("windows_security_read"))
            with self.assertRaisesRegex(WorkerError, "windows_security_read"):
                guard.opened(r"C:\fixture")
            guard.close()
            self.assertEqual(api.closed, [1])

    def test_each_foreign_write_replacement_right_refuses(self):
        for mask in (2, 4, 0x10, 0x40, 0x100, 0x10000, 0x40000, 0x80000, 0x10000000, 0x40000000):
            with self.subTest(mask=mask), self.assertRaisesRegex(WorkerError, "windows_foreign_authority"):
                worker_module.windows_acl_policy(self.security(aces=[(0, 0, mask, "S-1-1-0")]), "S-1-5-21-42")

    def test_ancestor_delete_child_and_inherited_write_are_checked(self):
        with self.assertRaisesRegex(WorkerError, "windows_foreign_authority"):
            worker_module.windows_acl_policy(self.security(aces=[(0, 0, 0x40, "S-1-1-0")]),
                                              "S-1-5-21-42", ancestor=True)
        with self.assertRaisesRegex(WorkerError, "windows_foreign_authority"):
            worker_module.windows_acl_policy(self.security(aces=[(0, 0x10, 2, "S-1-1-0")]), "S-1-5-21-42")
        worker_module.windows_acl_policy(self.security(aces=[(0, 0x8, 0x1f01ff, "S-1-1-0")]),
                                          "S-1-5-21-42", ancestor=True)

    def test_private_requires_protected_current_owner_and_no_foreign_access(self):
        for updates, code in (({"protected": False}, "windows_private_dacl"),
                              ({"owner": "S-1-5-18"}, "windows_private_dacl"),
                              ({"aces": [(0, 0, 0x80000000, "S-1-1-0")]}, "windows_foreign_authority")):
            with self.assertRaisesRegex(WorkerError, code):
                worker_module.windows_acl_policy(self.security(**updates), "S-1-5-21-42", private=True)

    def test_unknown_ace_or_flags_refuse_and_ordinary_deny_is_understood(self):
        for ace in ((5, 0, 1, "S-1-1-0"), (0, 0x20, 1, "S-1-1-0")):
            with self.assertRaisesRegex(WorkerError, "windows_ace_unsupported"):
                worker_module.windows_acl_policy(self.security(aces=[ace]), "S-1-5-21-42")
        worker_module.windows_acl_policy(self.security(aces=[(1, 0, 0x1f01ff, "S-1-1-0")]), "S-1-5-21-42")

    def test_hashes_actual_opened_bytes_and_holds_ancestors_until_close(self):
        with ExitStack() as stack:
            guard, api = self.guard(stack)
            path, digest, source = guard.file(r"C:\fixture\contract.py", collect=True)
            self.assertEqual(path, PureWindowsPath(r"C:\fixture\contract.py"))
            self.assertEqual(digest, hashlib.sha256(api.raw).hexdigest())
            self.assertEqual(source, api.raw)
            self.assertEqual(len(guard.objects), 3)
            self.assertEqual(api.closed, [])
            guard.close()
            guard.close()
        self.assertTrue(guard.closed)
        self.assertEqual(len(api.closed), 3)
        self.assertEqual(len(set(api.closed)), 3)

    def test_file_identity_descriptor_and_final_path_change_refuse(self):
        for field, value in (("signature", ("changed",)), ("final", r"C:\replacement")):
            with self.subTest(field=field), ExitStack() as stack:
                guard, api = self.guard(stack)
                guard.opened(r"C:\fixture")
                api.override = lambda facts: facts.update({field: value})
                with self.assertRaisesRegex(WorkerError, "windows_identity_changed"):
                    guard.revalidate()
                self.assertTrue(guard.lost)
                guard.close()

    def test_private_creation_attaches_descriptor_at_creation(self):
        with ExitStack() as stack:
            guard, api = self.guard(stack)
            stack.enter_context(patch.object(worker_module.tempfile, "gettempdir", return_value=r"C:\private"))
            stack.enter_context(patch.object(worker_module.secrets, "token_hex", return_value="supplied-nonce"))
            root = guard.create_private()
            self.assertEqual(root, PureWindowsPath(r"C:\private\jev-owned-supplied-nonce"))
            self.assertEqual(len(api.created), 1)
            self.assertTrue(api.created[0][1].startswith("O:S-1-5-21-42D:P"))
            self.assertIn("(A;OICI;FA;;;S-1-5-21-42)", api.created[0][1])
            guard.close(remove_cwd=True, cleanup_end=60)
        self.assertEqual(api.removed, [str(root)])
        self.assertTrue(guard.closed)
        self.assertIsNone(guard.created_directory)

    def test_identity_loss_prevents_directory_removal_but_closes_handles(self):
        with ExitStack() as stack:
            guard, api = self.guard(stack)
            guard.cwd = guard.opened(r"C:\private", directory=True, private=True)
            guard.created_directory = guard.cwd["path"]
            api.override = lambda facts: facts.update(signature=("replacement",))
            with self.assertRaisesRegex(WorkerError, "windows_identity_changed"):
                guard.close(remove_cwd=True, cleanup_end=60)
        self.assertEqual(api.removed, [])
        self.assertTrue(guard.closed)
        self.assertIsNotNone(guard.created_directory)
        self.assertTrue(guard.lost)

    def test_close_failure_is_unconfirmed_and_is_never_retried(self):
        with ExitStack() as stack:
            guard, api = self.guard(stack)
            guard.opened(r"C:\fixture")
            api.close_error = WorkerError("windows_handle_close")
            with self.assertRaisesRegex(WorkerError, "windows_handle_close"):
                guard.close()
            with self.assertRaisesRegex(WorkerError, "windows_close_unconfirmed"):
                guard.close()
        self.assertEqual(api.closed, [1])
        self.assertFalse(guard.closed)
        self.assertTrue(guard.lost)

    def test_creation_failure_retains_unknown_directory_without_unsafe_removal(self):
        with ExitStack() as stack:
            guard, api = self.guard(stack)
            stack.enter_context(patch.object(worker_module.tempfile, "gettempdir", return_value=r"C:\private"))
            api.create_directory = Mock(side_effect=WorkerError("windows_descriptor_close"))
            with self.assertRaisesRegex(WorkerError, "windows_descriptor_close") as created:
                guard.create_private()
            self.assertTrue(created.exception.facts["directory_creation_unconfirmed"])
            self.assertIsNotNone(guard.created_directory)
            with self.assertRaisesRegex(WorkerError, "windows_cleanup_identity"):
                guard.close(remove_cwd=True, cleanup_end=60)
        self.assertEqual(api.removed, [])
        self.assertTrue(guard.closed)
        self.assertIsNotNone(guard.created_directory)

    def test_unconfirmed_native_allocation_cannot_be_promoted_by_file_handle_closure(self):
        with ExitStack() as stack:
            guard, api = self.guard(stack)
            api.allocations, api.token_closed = {77}, True
            guard.opened(r"C:\fixture")
            with self.assertRaisesRegex(WorkerError, "windows_native_resource_unconfirmed"):
                guard.close()
        self.assertEqual(api.closed, [1])
        self.assertFalse(guard.closed)

    def test_original_deadline_prevents_new_object_and_cleanup_uses_only_original_reserve(self):
        clock = SimpleNamespace(value=0)
        with ExitStack() as stack:
            guard, api = self.guard(stack, clock=lambda: clock.value)
            guard.opened(r"C:\fixture")
            clock.value = 55
            with self.assertRaisesRegex(WorkerError, "deadline"):
                guard.opened(r"C:\late")
            guard.close(cleanup_end=60)
            self.assertEqual(guard.end, 55)
        self.assertEqual(len(api.handles), 1)
        self.assertTrue(guard.closed)

    def test_verified_contract_bytes_execute_without_second_path_open(self):
        source = b"supplied_value = 42\n"
        guard = Mock()
        guard.file.side_effect = [(PurePosixPath("/synthetic-helper.py"), "a", b""),
                                  (PurePosixPath("/synthetic-contract.py"), "b", source),
                                  (PurePosixPath("/synthetic-interpreter"), "c", b"")]
        header = {"worker_sha256": "a", "contract_sha256": "b", "interpreter_sha256": "c",
                  "interpreter": "/synthetic-interpreter", "cwd": "/synthetic-private", "hard_end": 60}
        with patch.object(worker_module.os, "name", "nt"), patch.object(worker_module, "WindowsTrust", return_value=guard), \
                patch.object(worker_module, "Path", PurePosixPath), \
                patch.object(worker_module, "WINDOWS_ACTIVE_TRUST", None), \
                patch.dict(worker_module.sys.modules), patch("builtins.open") as reopened:
            module = worker_module.load_contract(header, 55)
            self.assertEqual(module.supplied_value, 42)
            reopened.assert_not_called()
            guard.directory.assert_called_once_with("/synthetic-private", private=True)
            guard.runtime.assert_called_once()
            self.assertEqual(guard.revalidate.call_count, 2)

    def test_ntfs_opening_uses_pointer_width_handles_and_denies_write_delete_sharing(self):
        kernel, security = Mock(), Mock()
        kernel.CreateFileW.return_value = 77
        with patch.object(worker_module.ctypes, "WinDLL", side_effect=[kernel, security], create=True):
            api = worker_module.WindowsApi()
            self.assertEqual(api.open(r"C:\fixture"), 77)
            api.close(77)
        self.assertIs(kernel.CreateFileW.restype, worker_module.ctypes.c_void_p)
        self.assertEqual(kernel.CreateFileW.call_args.args[2], 1)
        self.assertEqual(kernel.CreateFileW.call_args.args[5], 0x02200000)
        self.assertIs(kernel.CloseHandle.restype, worker_module.ctypes.c_int32)
        self.assertIs(security.GetSecurityInfo.restype, worker_module.ctypes.c_uint32)
        kernel.CloseHandle.assert_called_once_with(77)

    def test_windows_environment_comes_from_native_system_directory_without_ambient_secrets(self):
        kernel = Mock()
        def directory(buffer, count):
            buffer.value = r"C:\Windows"
            return len(buffer.value)
        kernel.GetWindowsDirectoryW.side_effect = directory
        with patch.object(transport_module.ctypes, "WinDLL", return_value=kernel, create=True), \
                patch.dict(transport_module.os.environ, {"SystemRoot": "supplied-decoy", "HTTPS_PROXY": "supplied-decoy",
                                                       "JEV_TOKEN": "supplied-secret", "PYTHONPATH": "supplied-decoy"}):
            environment = transport_module.minimal_environment(platform="nt")
        self.assertEqual(environment, {"SystemRoot": r"C:\Windows"})
        self.assertIs(kernel.GetWindowsDirectoryW.restype, transport_module.ctypes.c_uint32)

    def test_windows_plan_keeps_original_interpreter_guard_and_unchanged_header_schema(self):
        guard = Mock(created_directory=None)
        guard.file.side_effect = [(PurePosixPath("/supplied-helper"), "a", b""), (PurePosixPath("/supplied-contract"), "b", b""),
                                  (PurePosixPath("/supplied-interpreter"), "c", b"")]
        guard.create_private.return_value = Path("/supplied-private")
        with patch.object(transport_module, "native_support", return_value="nt"), \
                patch.object(worker_module, "WindowsTrust", return_value=guard), \
                patch.object(transport_module, "minimal_environment", return_value={"SystemRoot": "supplied-root"}):
            plan = transport_module.prepare_plan(b"supplied wire", 55, 60, lambda: 0)
        self.assertIs(plan["windows_guard"], guard)
        self.assertEqual(plan["argv"], ["/supplied-interpreter", "-I", "-S", "-B", "/supplied-helper"])
        self.assertEqual((plan["header"]["operation_end"], plan["header"]["hard_end"]), (55, 60))
        self.assertEqual(set(plan["header"]), {"version", "operation_end", "hard_end", "endpoint", "wire_sha256",
                                             "worker_sha256", "contract_sha256", "interpreter", "interpreter_sha256", "cwd"})
        guard.close.assert_not_called()
        guard.revalidate.assert_called_once()

    def test_preparation_refusal_preserves_primary_and_independent_cleanup_facts(self):
        guard = Mock(created_directory=None, closed=False)
        guard.file.side_effect = WorkerError("windows_foreign_authority")
        guard.close.side_effect = WorkerError("windows_handle_close")
        with patch.object(transport_module, "native_support", return_value="nt"), \
                patch.object(worker_module, "WindowsTrust", return_value=guard):
            with self.assertRaisesRegex(TransportFailure, "windows_foreign_authority") as captured:
                transport_module.prepare_plan(b"supplied wire", 55, 60, lambda: 0)
        self.assertFalse(captured.exception.receipt["trust_handles_closed"])
        self.assertTrue(captured.exception.receipt["cwd_closed"])
        self.assertEqual(captured.exception.receipt["preparation_cleanup_error"], "windows_handle_close")

    def test_windows_trust_is_a_required_independent_completion_fact(self):
        facts = {key: True for key in ("start_finished", "created", "exit_observed", "reap_confirmed",
                 "stdout_complete", "stderr_complete", "stdout_closed", "stderr_closed",
                 "credential_channel_closed", "threads_joined", "handle_closed", "cwd_closed")}
        self.assertTrue(transport_module.settlement(facts))
        self.assertFalse(transport_module.settlement({**facts, "windows_trust_required": True}))
        self.assertTrue(transport_module.settlement({**facts, "windows_trust_required": True, "trust_handles_closed": True}))


class WindowsHandleFailureTests(unittest.TestCase):
    def authority(self, kernel, handle=None):
        if handle is None:
            class CapturedHandle(int):
                pass
            handle = CapturedHandle(77)
            handle.Close = Mock()
            handle.closed = False
        return transport_module.ProcessAuthority(SimpleNamespace(pid=999, returncode=None, _handle=handle), windows=True)

    def test_wait_failure_loses_authority_and_never_terminates_or_closes_handle(self):
        kernel = Mock()
        kernel.WaitForSingleObject.return_value = 0xFFFFFFFF
        with patch.object(transport_module.ctypes, "WinDLL", return_value=kernel, create=True):
            authority = self.authority(kernel)
            with self.assertRaisesRegex(TransportFailure, "exit_wait"):
                authority.wait_exact()
            self.assertFalse(authority.cancel())
            with self.assertRaisesRegex(TransportFailure, "handle_unconfirmed"):
                authority.close()
        self.assertTrue(authority.lost)
        kernel.TerminateProcess.assert_not_called()
        authority.handle.Close.assert_not_called()

    def test_close_exception_is_unconfirmed_and_never_retried(self):
        authority = self.authority(Mock())
        authority.reaped = True
        authority.handle.Close.side_effect = OSError("supplied close failure")
        with self.assertRaises(OSError):
            authority.close()
        with self.assertRaisesRegex(TransportFailure, "handle_unconfirmed"):
            authority.close()
        self.assertFalse(authority.cancel())
        authority.handle.Close.assert_called_once()
        self.assertTrue(authority.lost)
        self.assertFalse(authority.closed)

    def test_termination_failure_retains_original_handle_and_unsigned_exit_semantics(self):
        kernel = Mock()
        kernel.TerminateProcess.return_value = 0
        kernel.WaitForSingleObject.return_value = 0
        def exit_code(handle, pointer):
            pointer._obj.value = 0xC000013A
            return 1
        kernel.GetExitCodeProcess.side_effect = exit_code
        with patch.object(transport_module.ctypes, "WinDLL", return_value=kernel, create=True):
            authority = self.authority(kernel)
            with self.assertRaisesRegex(TransportFailure, "cancel_os"):
                authority.cancel()
            self.assertFalse(authority.cancel())
            self.assertEqual(authority.wait_exact(), 0xC000013A)
            authority.close()
        kernel.TerminateProcess.assert_called_once_with(77, 1)
        self.assertIs(kernel.TerminateProcess.restype, transport_module.ctypes.c_int32)
        self.assertIs(kernel.GetExitCodeProcess.restype, transport_module.ctypes.c_int32)

    def test_already_closed_captured_handle_is_refused_without_native_call(self):
        handle = Mock(closed=True)
        handle.__int__ = Mock(return_value=77)
        with patch.object(transport_module.ctypes, "WinDLL", create=True) as native:
            with self.assertRaisesRegex(TransportFailure, "captured_handle_unsupported"):
                self.authority(Mock(), handle)
        native.assert_not_called()



class WindowsNativeInspectionTests(unittest.TestCase):
    def api(self, kernel):
        api = object.__new__(worker_module.WindowsApi)
        api.owners, api.allocations, api.token_closed = [], set(), True
        api.kernel = kernel
        api.descriptor = Mock(return_value={"bytes": b"supplied descriptor"})
        kernel.GetFileType.return_value = 1
        kernel.GetDriveTypeW.return_value = 3
        def information(handle, kind, buffer, length):
            records = {0: worker_module.struct.pack("<qqqqI4x", 1, 2, 3, 4, 0),
                       1: worker_module.struct.pack("<qqIBB2x", 4096, 85, 1, 0, 0), 18: b"i" * 24}
            worker_module.ctypes.memmove(buffer, records[kind], length)
            return 1
        kernel.GetFileInformationByHandleEx.side_effect = information
        def volume(handle, name, length, serial, maximum, flags, filesystem, count):
            flags._obj.value = 8
            filesystem.value = "NTFS"
            return 1
        kernel.GetVolumeInformationByHandleW.side_effect = volume
        def final(handle, buffer, count, flags):
            buffer.value = "\\\\?\\C:\\supplied\\file"
            return len(buffer.value)
        kernel.GetFinalPathNameByHandleW.side_effect = final
        return api

    def test_ntfs_identity_is_captured_without_decoding_pointer_width_as_dword(self):
        kernel = Mock()
        facts = self.api(kernel).inspect(77)
        self.assertEqual(facts["identity"], b"i" * 24)
        self.assertEqual(facts["size"], 85)
        self.assertFalse(facts["directory"])
        self.assertEqual(facts["final"], r"C:\supplied\file")
        self.assertEqual(kernel.GetFileInformationByHandleEx.call_count, 3)

    def test_device_network_path_reparse_and_remote_filesystem_refuse(self):
        for kind in ("device", "network", "reparse", "filesystem"):
            with self.subTest(kind=kind):
                kernel = Mock()
                api = self.api(kernel)
                if kind == "device":
                    kernel.GetFileType.return_value = 3
                elif kind == "network":
                    def network(handle, buffer, count, flags):
                        buffer.value = r"\\?\UNC\server\supplied"
                        return len(buffer.value)
                    kernel.GetFinalPathNameByHandleW.side_effect = network
                elif kind == "reparse":
                    original = kernel.GetFileInformationByHandleEx.side_effect
                    def reparse(handle, info, buffer, length):
                        result = original(handle, info, buffer, length)
                        if info == 0:
                            worker_module.struct.pack_into("<I", buffer, 32, 0x400)
                        return result
                    kernel.GetFileInformationByHandleEx.side_effect = reparse
                else:
                    kernel.GetVolumeInformationByHandleW.side_effect = None
                    kernel.GetVolumeInformationByHandleW.return_value = 0
                with self.assertRaises(WorkerError):
                    api.inspect(77)


class WindowsHarnessTests(unittest.TestCase):
    def test_platform_prerequisite_refuses_without_listener_or_process(self):
        with patch.object(transport_module, "native_support", return_value="posix"), \
                patch.object(transport_module, "prepare_plan") as prepare, \
                patch.object(local_module, "LoopbackFixture") as listener:
            report = local_module.run_trust_case("preflight")
        self.assertEqual(report["oracle"], "not_run")
        self.assertEqual(report["native_control"], "NOT RUN")
        self.assertEqual(report["error"], "windows_native_required")
        self.assertFalse(report["listener_opened"])
        self.assertEqual((report["helper_count"], report["requests"], report["responses"]), (0, 0, 0))
        prepare.assert_not_called()
        listener.assert_not_called()

    def test_preparation_refusal_preserves_owned_failure_receipt(self):
        original = {"trust_handles_closed": False, "cwd_closed": False, "preparation_cleanup_error": "supplied cleanup"}
        with patch.object(transport_module, "native_support", return_value="nt"), \
                patch.object(transport_module, "prepare_plan", side_effect=TransportFailure("windows_dacl", original)), \
                patch.object(local_module, "LoopbackFixture") as listener:
            report = local_module.run_trust_case("preflight")
        self.assertEqual(report["oracle"], "not_run")
        self.assertEqual(report["error"], "windows_dacl")
        self.assertEqual(report["preparation"], original)
        self.assertFalse(report["listener_opened"])
        listener.assert_not_called()

    def test_successful_preflight_records_exact_plan_and_independent_cleanup(self):
        guard = Mock(closed=False, created_directory="supplied cwd")
        guard.receipt.return_value = {"objects": ["supplied identity"]}
        def close(**kwargs):
            guard.closed, guard.created_directory = True, None
        guard.close.side_effect = close
        plan = {"header": {"interpreter": "supplied interpreter", "interpreter_sha256": "a" * 64,
                           "worker_sha256": "b" * 64, "contract_sha256": "c" * 64}, "windows_guard": guard}
        with patch.object(transport_module, "native_support", return_value="nt"), \
                patch.object(transport_module, "prepare_plan", return_value=plan), \
                patch.object(local_module.time, "monotonic", return_value=0), \
                patch.object(local_module, "LoopbackFixture") as listener:
            report = local_module.run_trust_case("preflight")
        self.assertEqual(report["oracle"], "satisfied")
        self.assertEqual(report["selected_interpreter"], "supplied interpreter")
        self.assertEqual(report["runtime"]["trust"], {"objects": ["supplied identity"]})
        self.assertTrue(report["runtime"]["trust_handles_closed"] and report["runtime"]["cwd_closed"])
        guard.close.assert_called_once_with(remove_cwd=True, cleanup_end=60)
        listener.assert_not_called()

    def test_failed_preflight_cleanup_does_not_infer_directory_closure_from_handles(self):
        guard = Mock(closed=True, created_directory="supplied retained cwd")
        guard.receipt.return_value = {"objects": ["supplied identity"]}
        guard.close.side_effect = WorkerError("windows_directory_remove")
        plan = {"header": {"interpreter": "supplied interpreter", "interpreter_sha256": "a" * 64,
                           "worker_sha256": "b" * 64, "contract_sha256": "c" * 64}, "windows_guard": guard}
        with patch.object(transport_module, "native_support", return_value="nt"), \
                patch.object(transport_module, "prepare_plan", return_value=plan):
            report = local_module.run_trust_case("preflight")
        self.assertNotEqual(report["oracle"], "satisfied")
        self.assertEqual(report["error"], "windows_directory_remove")
        self.assertTrue(report["runtime"]["trust_handles_closed"])
        self.assertFalse(report["runtime"]["cwd_closed"])
        guard.close.assert_called_once()



class WindowsCompletionTests(unittest.TestCase):
    def test_actual_lease_finish_requires_trust_and_directory_closure_independently(self):
        for failure in (False, True):
            with self.subTest(failure=failure):
                guard = Mock(closed=False, created_directory="supplied cwd", lost=False)
                def close(**kwargs):
                    guard.closed = True
                    if failure:
                        guard.lost = True
                        raise WorkerError("windows_identity_changed")
                    guard.created_directory = None
                guard.close.side_effect = close
                lease = transport_module.OwnedLease({"cwd": PurePosixPath("/supplied cwd"), "header": {},
                                                    "windows_guard": guard}, 55, 60, clock=lambda: 0)
                lease.authority = Mock(lost=False)
                lease.facts.update({key: True for key in ("start_finished", "created", "exit_observed", "reap_confirmed",
                                   "stdout_complete", "stderr_complete", "stdout_closed", "stderr_closed",
                                   "credential_channel_closed")})
                with patch.object(transport_module.shutil, "rmtree") as unsafe_remove:
                    receipt = lease.finish()
                unsafe_remove.assert_not_called()
                guard.close.assert_called_once_with(remove_cwd=True, cleanup_end=60)
                self.assertTrue(receipt["handle_closed"] and receipt["trust_handles_closed"])
                self.assertEqual(receipt["cwd_closed"], not failure)
                self.assertEqual(receipt["completion_confirmed"], not failure)
                if failure:
                    self.assertEqual(receipt["error"], "cleanup_unconfirmed")
                    self.assertEqual(receipt["primary_error"], "windows_identity_changed")
                    self.assertTrue(receipt["ownership_lost"])

    def test_actual_helper_receipt_cannot_succeed_after_trust_handle_close_failure(self):
        for failure in (False, True):
            with self.subTest(failure=failure), ExitStack() as stack:
                guard = Mock(closed=not failure, hard_end=60)
                if failure:
                    guard.close.side_effect = WorkerError("windows_handle_close")
                stack.enter_context(patch.object(worker_module, "WINDOWS_ACTIVE_TRUST", guard))
                header = {"operation_end": 55, "hard_end": 60, "wire_sha256": "a" * 64}
                supplied_contract = Mock()
                supplied_contract.validate_answers.return_value = {"q": {"type": "choice", "choice": "x",
                                                                        "probabilities": {"x": 1}}}
                stack.enter_context(patch.object(worker_module, "helper_input", return_value=(
                    header, b"supplied wire", {"questions": {"q": {}}}, b"synthetic token", supplied_contract)))
                stack.enter_context(patch.object(worker_module, "https_exchange", return_value=(b"{}", {"socket_closed": True})))
                stack.enter_context(patch.object(worker_module, "remaining", side_effect=lambda end: end))
                output = Mock()
                def write(raw):
                    output.raw = raw
                    return len(raw)
                output.buffer.write.side_effect = write
                stack.enter_context(patch.object(worker_module.sys, "stdout", output))
                stack.enter_context(patch.object(worker_module.sys, "stderr", Mock()))
                stack.enter_context(patch.object(worker_module.sys, "stdin", Mock()))
                result = worker_module.main()
                receipt = json.loads(output.raw)
                self.assertEqual(result, int(failure))
                self.assertEqual(receipt["windows_trust_closed"], not failure)
                self.assertEqual(receipt["status"], "failed" if failure else "completed")
                if failure:
                    self.assertEqual(receipt["error"], "windows_handle_close")
                    self.assertNotIn("answers", receipt)
                else:
                    self.assertEqual(receipt["answers"], supplied_contract.validate_answers.return_value)
                guard.close.assert_called_once_with(cleanup_end=60)



class WindowsAcquisitionRegressionTests(unittest.TestCase):
    def native_api(self, stack, *, failed=False):
        kernel, security = Mock(), Mock()
        security.IsValidSid.return_value = 1
        security.GetLengthSid.return_value = 16
        def token(process, access, output):
            output._obj.value = 77
            return 1
        security.OpenProcessToken.side_effect = token
        def information(handle, kind, buffer, length, output):
            output._obj.value = 16
            if buffer is not None:
                worker_module.ctypes.c_void_p.from_buffer(buffer).value = 123
            return int(buffer is not None)
        security.GetTokenInformation.side_effect = information
        backing = worker_module.ctypes.create_unicode_buffer("S-1-5-21-42")
        def sid(pointer, output):
            output._obj.value = 99 if failed else worker_module.ctypes.addressof(backing)
            return int(not failed)
        security.ConvertSidToStringSidW.side_effect = sid
        kernel.LocalFree.return_value = 99 if failed else None
        kernel.CloseHandle.return_value = int(not failed)
        stack.enter_context(patch.object(worker_module.ctypes, "WinDLL", side_effect=[kernel, security], create=True))
        api = worker_module.WindowsApi()
        api.synthetic_backing = backing
        return api, kernel

    def test_sid_failure_survives_partial_construction_and_ambiguous_closes(self):
        for boundary in ("api", "prepare", "identity", "worker"):
            with self.subTest(boundary=boundary), ExitStack() as stack:
                api, kernel = self.native_api(stack, failed=True)
                stack.enter_context(patch.object(worker_module, "WindowsApi", return_value=api))
                stack.enter_context(patch.object(worker_module.os, "name", "nt"))
                stack.enter_context(patch.object(worker_module.time, "monotonic", return_value=0))
                stack.enter_context(patch.object(worker_module, "remaining", side_effect=lambda end, *args: end))
                stack.enter_context(patch.object(transport_module, "native_support", return_value="nt"))
                stack.enter_context(patch.object(worker_module, "WINDOWS_ACTIVE_TRUST", None))
                invoke = {"api": api.current_user,
                          "prepare": lambda: transport_module.prepare_plan(b"supplied wire", 55, 60, lambda: 0),
                          "identity": lambda: transport_module.identity_hash("supplied path", 55, lambda: 0),
                          "worker": lambda: worker_module.load_contract({"hard_end": 60}, 55)}[boundary]
                with self.assertRaises((WorkerError, TransportFailure)) as caught:
                    invoke()
                self.assertEqual(caught.exception.code, "windows_sid")
                kernel.CloseHandle.assert_called_once()
                self.assertEqual(kernel.CloseHandle.call_args.args[0].value, 77)
                kernel.LocalFree.assert_called_once()
                self.assertFalse(api.token_closed)
                self.assertEqual(api.allocations, {99})
                owner = getattr(caught.exception, "owner", None)
                self.assertIsNotNone(owner)
                owned_api = owner if owner is api else owner.api
                self.assertIs(owned_api, api)
                resources = api.receipt()["owned_resources"]
                self.assertTrue(any(item["kind"] == "token" and item["identity"] == 77 and
                                    item["release_attempted"] and not item["closed"] for item in resources))
                self.assertTrue(any(item["kind"] == "allocation" and item["identity"] == 99 and
                                    item["release_attempted"] and not item["closed"] for item in resources))

    def test_control_inspection_failure_closes_known_handle_once_and_retains_unknown_path(self):
        for close_failure in (False, True):
            with self.subTest(close_failure=close_failure), ExitStack() as stack:
                api = WindowsApiFixture()
                guards = []
                original_type = worker_module.WindowsTrust
                stack.enter_context(patch.object(worker_module, "Path", PureWindowsPath))
                stack.enter_context(patch.object(worker_module.os.path, "abspath", side_effect=lambda path: str(path)))
                stack.enter_context(patch.object(PureWindowsPath, "exists", return_value=False, create=True))
                stack.enter_context(patch.object(worker_module.tempfile, "gettempdir", return_value=r"C:\private"))
                stack.enter_context(patch.object(worker_module.secrets, "token_hex", return_value="supplied-nonce"))
                stack.enter_context(patch.object(local_module.time, "monotonic", return_value=0))
                def guard_factory(end):
                    guard = original_type(end, api=api, clock=lambda: 0)
                    guards.append(guard)
                    return guard
                stack.enter_context(patch.object(worker_module, "WindowsTrust", side_effect=guard_factory))
                stack.enter_context(patch.object(transport_module, "native_support", return_value="nt"))
                prepared = Mock(closed=False, created_directory="supplied cwd")
                prepared.receipt.return_value = {"objects": []}
                def close_prepared(**kwargs):
                    prepared.closed, prepared.created_directory = True, None
                prepared.close.side_effect = close_prepared
                plan = {"header": {"interpreter": "supplied interpreter", "interpreter_sha256": "a" * 64,
                                   "worker_sha256": "b" * 64, "contract_sha256": "c" * 64}, "windows_guard": prepared}
                stack.enter_context(patch.object(transport_module, "prepare_plan", return_value=plan))
                inspect, close = api.inspect, api.close
                def inspect_control(handle):
                    if api.handles[handle]["path"].endswith("\\control"):
                        raise WorkerError("windows_file_identity")
                    return inspect(handle)
                def close_control(handle):
                    close(handle)
                    if close_failure and api.handles[handle]["path"].endswith("\\control"):
                        raise WorkerError("windows_handle_close")
                api.inspect, api.close = inspect_control, close_control
                report = local_module.run_trust_case("trusted_directory")
                handles = [handle for handle, facts in api.handles.items() if facts["path"].endswith("\\control")]
                self.assertEqual(report["error"], "windows_file_identity")
                self.assertEqual(len(handles), 1)
                self.assertEqual(api.closed.count(handles[0]), 1)
                self.assertFalse(any(path.endswith("\\control") for path in api.removed))
                self.assertNotEqual(report["oracle"], "satisfied")
                self.assertTrue(report["fixture"]["unconfirmed_paths"])
                if close_failure:
                    self.assertTrue(report["cleanup_errors"])
                    self.assertTrue(any(entry["handle"] == handles[0] and entry.get("close_attempted") and
                                        not entry["closed"] for guard in guards for entry in guard.objects))

    def test_successful_construction_and_token_settlement_remain_positive(self):
        with ExitStack() as stack:
            api, kernel = self.native_api(stack)
            guard = worker_module.WindowsTrust(55, clock=lambda: 0, api=api)
            self.assertEqual(guard.user, "S-1-5-21-42")
            guard.close()
            guard.close()
            self.assertTrue(guard.closed)
            self.assertTrue(api.token_closed)
            self.assertEqual(api.allocations, set())
            kernel.CloseHandle.assert_called_once()
            kernel.LocalFree.assert_called_once()



class WindowsAcquisitionRepairTests(unittest.TestCase):
    def test_reparse_creation_failure_retains_numeric_cause_and_unknown_paths(self):
        with ExitStack() as stack:
            api, guards = self.fixture(stack)
            error = OSError(13, "opaque path-bearing detail", "opaque private path")
            error.winerror = 1314
            stack.enter_context(patch.object(local_module, "create_junction", side_effect=error))
            report = local_module.run_trust_case("reparse_refused")
        self.assertEqual(report["error"], "windows_reparse_control_unavailable")
        self.assertEqual(report["preparation"], {"reparse_creation_os": {"errno": 13, "winerror": 1314}})
        self.assertEqual(report["native_control"], "NOT RUN")
        self.assertEqual(len(report["fixture"]["unconfirmed_paths"]), 2)
        self.assertFalse(report["fixture"]["unconfirmed_paths"][-1]["creation_confirmed"])
        self.assertFalse(report["fixture"]["root_closed"])
        self.assertTrue(all(guard.closed for guard in guards))
        self.assertFalse(any(path.endswith("\\target") for path in api.removed))

    def test_inherited_control_uses_default_creation_and_requires_observed_inheritance(self):
        for inherited in (True, False):
            with self.subTest(inherited=inherited), ExitStack() as stack:
                api, guards = self.fixture(stack)
                mkdir = stack.enter_context(patch.object(local_module.os, "mkdir"))
                def inherited_security(facts):
                    if facts["final"].endswith("\\inheriting\\control") and inherited:
                        facts["security"]["aces"].append((0, 0x10, 2, "S-1-1-0"))
                api.override = inherited_security
                report = local_module.run_trust_case("inherited_write")
                mkdir.assert_called_once()
                self.assertTrue(str(mkdir.call_args.args[0]).endswith("\\inheriting\\control"))
                self.assertFalse(any(path.endswith("\\control") for path, _ in api.created))
                self.assertIn("inheritance_parent_security", report["fixture"])
                self.assertIn("inheritance_child_security", report["fixture"])
                if inherited:
                    self.assertEqual(report["oracle"], "satisfied")
                    self.assertEqual(report["fixture"]["observed_predicate"], "windows_foreign_authority")
                else:
                    self.assertEqual(report["error"], "windows_inherited_control_unavailable")
                    self.assertEqual(report["native_control"], "NOT RUN")
                self.assertTrue(all(guard.closed for guard in guards))
                self.assertEqual(report["cleanup_errors"], [])

    def fixture(self, stack):
        api, guards = WindowsApiFixture(), []
        original = worker_module.WindowsTrust
        stack.enter_context(patch.object(worker_module, "Path", PureWindowsPath))
        stack.enter_context(patch.object(worker_module.os.path, "abspath", side_effect=lambda path: str(path)))
        stack.enter_context(patch.object(worker_module.tempfile, "gettempdir", return_value=r"C:\private"))
        stack.enter_context(patch.object(worker_module.secrets, "token_hex", return_value="supplied-nonce"))
        stack.enter_context(patch.object(local_module.time, "monotonic", return_value=0))
        def factory(end):
            guard = original(end, api=api, clock=lambda: 0)
            guards.append(guard)
            return guard
        stack.enter_context(patch.object(worker_module, "WindowsTrust", side_effect=factory))
        stack.enter_context(patch.object(transport_module, "native_support", return_value="nt"))
        prepared = Mock(closed=False, created_directory="supplied cwd")
        prepared.receipt.return_value = {"objects": []}
        def close_prepared(**kwargs):
            prepared.closed, prepared.created_directory = True, None
        prepared.close.side_effect = close_prepared
        plan = {"header": {"interpreter": "supplied interpreter", "interpreter_sha256": "a" * 64,
                           "worker_sha256": "b" * 64, "contract_sha256": "c" * 64}, "windows_guard": prepared}
        stack.enter_context(patch.object(transport_module, "prepare_plan", return_value=plan))
        return api, guards

    def test_successful_control_uses_registered_original_handles_and_removes_owned_paths(self):
        with ExitStack() as stack:
            api, guards = self.fixture(stack)
            report = local_module.run_trust_case("trusted_directory")
        self.assertEqual(report["oracle"], "satisfied")
        self.assertIsNone(report["error"])
        self.assertEqual(report["cleanup_errors"], [])
        self.assertEqual(report["fixture"]["unconfirmed_paths"], [])
        self.assertTrue(report["fixture"]["root_closed"])
        self.assertTrue(all(guard.closed for guard in guards))
        self.assertEqual(len(api.closed), len(set(api.closed)))
        self.assertEqual(set(api.closed), set(api.handles))
        self.assertEqual(len([path for path in api.removed if path.endswith("\\control")]), 1)
        self.assertEqual((report["operation_end"], report["hard_end"], report["run_end"]), (55, 60, 600))
        self.assertEqual((report["helper_count"], report["requests"], report["listener_opened"]), (0, 0, False))

    def test_adjacent_created_objects_are_closed_once_after_inspection_failure(self):
        for case, suffix in (("inherited_write", "\\inheriting"), ("reparse_refused", "\\target")):
            with self.subTest(case=case), ExitStack() as stack:
                api, _ = self.fixture(stack)
                inspect = api.inspect
                def fail(handle):
                    if api.handles[handle]["path"].endswith(suffix):
                        raise WorkerError("windows_file_identity")
                    return inspect(handle)
                api.inspect = fail
                report = local_module.run_trust_case(case)
                acquired = [handle for handle, value in api.handles.items() if value["path"].endswith(suffix)]
                self.assertEqual(len(acquired), 1)
                self.assertEqual(api.closed.count(acquired[0]), 1)
                self.assertEqual(report["error"], "windows_file_identity")
                self.assertTrue(report["fixture"]["unconfirmed_paths"])
                self.assertFalse(any(path.endswith(suffix) for path in api.removed))
                self.assertNotEqual(report["oracle"], "satisfied")

    def test_created_path_without_opened_identity_is_retained_without_removal(self):
        with ExitStack() as stack:
            api, _ = self.fixture(stack)
            opening = api.open
            def fail(path, **kwargs):
                if str(path).endswith("\\control"):
                    raise WorkerError("windows_open")
                return opening(path, **kwargs)
            api.open = fail
            report = local_module.run_trust_case("trusted_directory")
        self.assertEqual(report["error"], "windows_open")
        self.assertTrue(report["fixture"]["root_handles_closed"])
        self.assertFalse(report["fixture"]["root_closed"])
        self.assertEqual(api.removed, [])
        pending = report["fixture"]["unconfirmed_paths"][0]
        self.assertTrue(pending["creation_confirmed"])
        self.assertFalse(pending["identity_confirmed"])
        self.assertIsNone(pending["handle"])

    def test_partial_probe_constructor_blocks_fixture_removal_and_retains_its_owner(self):
        with ExitStack() as stack:
            api, guards = self.fixture(stack)
            original = worker_module.WindowsTrust.side_effect
            owner = SimpleNamespace(closed=False)
            failure = WorkerError("windows_token")
            failure.owner = owner
            def factory(end):
                if guards:
                    raise failure
                return original(end)
            worker_module.WindowsTrust.side_effect = factory
            report = local_module.run_trust_case("trusted_directory")
        self.assertEqual(report["error"], "windows_token")
        self.assertEqual(api.removed, [])
        self.assertTrue(report["fixture"]["root_handles_closed"])
        self.assertFalse(report["fixture"]["root_closed"])
        self.assertIs(worker_module.WINDOWS_UNCONFIRMED_OWNERS[id(owner)], owner)

    def test_native_information_failure_preserves_primary_and_ambiguous_token_owner(self):
        with ExitStack() as stack:
            api, kernel = WindowsAcquisitionRegressionTests().native_api(stack)
            api.security.GetTokenInformation.side_effect = OSError("synthetic native failure")
            kernel.CloseHandle.return_value = 0
            with self.assertRaises(WorkerError) as caught:
                api.current_user()
            self.assertEqual(caught.exception.code, "windows_native_os")
            self.assertEqual(caught.exception.facts["cleanup_errors"], ["windows_handle_close"])
            self.assertIs(caught.exception.owner, api)
            with self.assertRaises(WorkerError):
                api.settle_known()
            with self.assertRaisesRegex(WorkerError, "windows_native_resource_unconfirmed"):
                api.current_user()
            kernel.CloseHandle.assert_called_once()
            api.security.OpenProcessToken.assert_called_once()
            self.assertFalse(api.receipt()["all_resources_closed"])

    def test_failed_sid_with_confirmed_cleanup_retains_primary_and_settled_owner(self):
        with ExitStack() as stack:
            api, kernel = WindowsAcquisitionRegressionTests().native_api(stack, failed=True)
            kernel.LocalFree.return_value = None
            kernel.CloseHandle.return_value = 1
            with self.assertRaises(WorkerError) as caught:
                worker_module.WindowsTrust(55, api=api, clock=lambda: 0)
            self.assertEqual(caught.exception.code, "windows_sid")
            self.assertTrue(caught.exception.owner.closed)
            self.assertTrue(caught.exception.facts["windows_trust"]["native_resources"]["all_resources_closed"])
            self.assertFalse(caught.exception.facts["constructor_complete"])
            kernel.LocalFree.assert_called_once()
            kernel.CloseHandle.assert_called_once()

    def test_worker_primary_failure_and_independent_cleanup_survive_json_receipt(self):
        with ExitStack() as stack:
            guard = Mock(closed=False, hard_end=60)
            guard.close.side_effect = WorkerError("windows_handle_close")
            guard.receipt.return_value = {"handles_closed": False, "native_resources": {
                "owned_resources": [{"kind": "token", "identity": 77, "release_attempted": True, "closed": False}],
                "all_resources_closed": False}}
            stack.enter_context(patch.object(worker_module, "WINDOWS_ACTIVE_TRUST", guard))
            stack.enter_context(patch.object(worker_module, "helper_input", side_effect=WorkerError("windows_sid")))
            stack.enter_context(patch.object(worker_module.sys, "stdin", Mock()))
            stack.enter_context(patch.object(worker_module.sys, "stderr", Mock()))
            output = Mock()
            def write(raw):
                output.raw = raw
                return len(raw)
            output.buffer.write.side_effect = write
            stack.enter_context(patch.object(worker_module.sys, "stdout", output))
            self.assertEqual(worker_module.main(), 1)
            report = json.loads(output.raw)
        self.assertEqual(report["error"], "windows_sid")
        self.assertEqual(report["cleanup_errors"], ["windows_handle_close"])
        self.assertFalse(report["windows_trust_closed"])
        self.assertEqual(report["native_resources"]["owned_resources"][0]["identity"], 77)
        self.assertNotIn("answers", report)
        guard.close.assert_called_once_with(cleanup_end=60)


    def test_api_capture_settles_handle_when_guard_registration_fails(self):
        with ExitStack() as stack:
            api, kernel = WindowsAcquisitionRegressionTests().native_api(stack)
            kernel.CreateFileW.return_value = 88
            guard, _ = WindowsTrustTests().guard(stack, api=api)
            class BrokenRecords(list):
                def append(self, value):
                    raise MemoryError("synthetic ownership registration failure")
            guard.objects = BrokenRecords()
            with self.assertRaises(MemoryError):
                guard.capture(r"C:\fixture")
            guard.close()
            guard.close()
            self.assertTrue(guard.closed)
            self.assertTrue(api.settled())
            self.assertEqual(sum(call.args == (88,) for call in kernel.CloseHandle.call_args_list), 1)
            self.assertTrue(api.record("file", 88)["closed"])

    def test_descriptor_and_creation_allocations_preserve_primary_on_failed_free(self):
        for boundary, expected in (("descriptor", "windows_security_read"),
                                   ("creation", "windows_creation_descriptor")):
            with self.subTest(boundary=boundary), ExitStack() as stack:
                api, kernel = WindowsAcquisitionRegressionTests().native_api(stack)
                kernel.LocalFree.return_value = 99
                def descriptor(*args):
                    args[-1]._obj.value = 99
                    return 5
                def creation(source, version, output, length):
                    output._obj.value = 99
                    return 0
                api.security.GetSecurityInfo.side_effect = descriptor
                api.security.ConvertStringSecurityDescriptorToSecurityDescriptorW.side_effect = creation
                invoke = (lambda: api.descriptor(88)) if boundary == "descriptor" else \
                    (lambda: api.create_directory("supplied path", "supplied descriptor"))
                with self.assertRaises(WorkerError) as caught:
                    invoke()
                self.assertEqual(caught.exception.code, expected)
                self.assertEqual(caught.exception.facts["cleanup_errors"], ["windows_descriptor_close"])
                self.assertIs(caught.exception.owner, api)
                with self.assertRaises(WorkerError):
                    api.settle_known()
                kernel.LocalFree.assert_called_once()
                kernel.CreateDirectoryW.assert_not_called()
                resource = api.receipt()["owned_resources"][0]
                self.assertEqual((resource["identity"], resource["release_attempted"], resource["closed"]), (99, True, False))


    def test_parent_preserves_failed_helper_owner_and_blocks_next_lease(self):
        wire = contract.encode({"state": "synthetic ownership fixture", "model": "fixture-alias", "questions": {
            "q": {"type": "choice", "instructions": "opaque", "criteria": {"x": "opaque"}}}})
        owner = {"kind": "token", "identity": 77, "release_attempted": True, "closed": False}
        lease = Mock(cancelled=False, facts={"start_finished": True, "error": None})
        lease.stdout = contract.encode({"version": 1, "status": "failed", "error": "windows_sid",
            "credential_channel_closed": True, "windows_trust_closed": False,
            "native_resources": {"owned_resources": [owner], "all_resources_closed": False}})
        lease.finish.return_value = {"completion_confirmed": True, "error": None, "returncode": 1,
                                    "exit_observed": True, "reap_confirmed": True}
        plan = Mock(return_value={"windows_guard": Mock(), "header": {
            "interpreter_sha256": "a" * 64, "worker_sha256": "b" * 64}})
        transport = transport_module.JevTransport(clock=lambda: 0, plan_factory=plan,
                                                  lease_factory=Mock(return_value=lease))
        with self.assertRaises(TransportFailure) as caught:
            transport.send(wire, token="synthetic-token")
        self.assertEqual(caught.exception.code, "windows_sid")
        report = caught.exception.receipt
        self.assertEqual(report["helper"]["native_resources"]["owned_resources"], [owner])
        self.assertTrue(report["process_completion_confirmed"] and report["exit_observed"] and report["reap_confirmed"])
        self.assertFalse(report["completion_confirmed"])
        self.assertTrue(report["helper_trust_unconfirmed"])
        with self.assertRaisesRegex(TransportFailure, "lease_unavailable"):
            transport.send(wire, token="synthetic-token")
        plan.assert_called_once()
        lease.start.assert_called_once()
        lease.cancel.assert_not_called()


if __name__ == "__main__":
    unittest.main()
