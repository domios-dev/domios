"""Synthetic public-trust inputs and typed errors; no real TLS context or OS CA parse."""

import hashlib
import json
from pathlib import Path
import ssl
import stat
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import urllib.error

import acquisition_transport as parent
import acquisition_worker as worker
from bounded_io_tests import Clock, FakeOpener, FakeResponse

PEM = b"-----BEGIN CERTIFICATE-----\nc3VwcGxpZWQgb3BhcXVlIHRlc3QgYnl0ZXM=\n-----END CERTIFICATE-----\n"


def source_identity(raw=PEM):
    return {"dev": 1, "ino": 2, "uid": 0, "gid": 0, "mode": stat.S_IFREG | 0o644,
            "bytes": len(raw), "mtime_ns": 3}


class TLSControls(unittest.TestCase):
    def setUp(self):
        self.clock = Clock()
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.workspace = self.root / "supplied.tgz.acquisition"
        self.workspace.mkdir()
        self.snapshot = self.workspace / "public-ca.pem"
        self.snapshot.write_bytes(PEM)
        self.snapshot.chmod(0o400)
        self.identity = source_identity()
        self.resolved = Path("/private/etc/ssl/cert.pem")
        source = patch.object(worker, "public_ca_source", return_value=(self.resolved, self.identity))
        source.start()
        self.addCleanup(source.stop)
        self.tls = {"platform": "darwin", "mode": "os_public_ca_snapshot", "source": "/etc/ssl/cert.pem",
                    "resolved_source": str(self.resolved), "source_identity": self.identity.copy(),
                    "snapshot": str(self.snapshot), "snapshot_identity": worker.stat_identity(self.snapshot.stat()),
                    "bytes": len(PEM), "sha256": hashlib.sha256(PEM).hexdigest()}
        self.request = {"fixture_root": str(self.root), "receipt": str(self.workspace / "receipt.json"), "tls": self.tls}
        self.context = SimpleNamespace(verify_mode=ssl.CERT_REQUIRED, check_hostname=True, keylog_filename=None,
                                       cert_store_stats=lambda: {"x509_ca": 1})
        self.factory_calls = []

    def validate(self):
        return worker.validated_tls(self.request, 10, self.clock, platform="darwin")

    def factory(self, **kwargs):
        self.factory_calls.append(kwargs)
        return self.context

    def context_for(self):
        return worker.tls_context(self.request, 10, self.clock, platform="darwin", factory=self.factory)

    def test_valid_snapshot_identity_path_and_hash_control(self):
        self.assertEqual(self.validate(), self.snapshot)

    def test_snapshot_hash_mismatch_is_refused(self):
        self.tls["sha256"] = "0" * 64
        with self.assertRaisesRegex(worker.AcquisitionError, "identity/hash differs"):
            self.validate()

    def test_snapshot_identity_mismatch_is_refused(self):
        self.tls["snapshot_identity"]["ino"] += 1
        with self.assertRaisesRegex(worker.AcquisitionError, "identity/hash differs"):
            self.validate()

    def test_source_identity_mismatch_is_refused(self):
        self.tls["source_identity"]["ino"] += 1
        with self.assertRaisesRegex(worker.AcquisitionError, "source identity differs"):
            self.validate()

    def test_resolved_source_path_mismatch_is_refused(self):
        self.tls["resolved_source"] = "/supplied/foreign-source"
        with self.assertRaisesRegex(worker.AcquisitionError, "source identity differs"):
            self.validate()

    def test_arbitrary_source_path_is_refused(self):
        self.tls["source"] = "/supplied/arbitrary.pem"
        with self.assertRaisesRegex(worker.AcquisitionError, "binding differs"):
            self.validate()

    def test_different_snapshot_path_is_refused(self):
        self.tls["snapshot"] = str(self.workspace / "other.pem")
        with self.assertRaisesRegex(worker.AcquisitionError, "snapshot path differs"):
            self.validate()

    def test_escaped_snapshot_path_is_refused(self):
        self.tls["snapshot"] = str(self.root.parent / "foreign.pem")
        with self.assertRaisesRegex(worker.AcquisitionError, "escaped fixture"):
            self.validate()

    def test_absent_snapshot_is_refused(self):
        self.snapshot.chmod(0o600)
        self.snapshot.unlink()
        with self.assertRaises(FileNotFoundError):
            self.validate()

    def test_empty_snapshot_is_refused(self):
        self.snapshot.chmod(0o600)
        self.snapshot.write_bytes(b"")
        with self.assertRaisesRegex(worker.AcquisitionError, "unsafe or empty"):
            self.validate()

    def test_malformed_snapshot_is_refused(self):
        self.snapshot.chmod(0o600)
        self.snapshot.write_bytes(b"supplied non-PEM bytes")
        with self.assertRaisesRegex(worker.AcquisitionError, "PEM structure invalid"):
            self.validate()

    def test_private_key_material_is_refused(self):
        with self.assertRaisesRegex(worker.AcquisitionError, "PEM structure invalid"):
            worker.public_ca_bytes(b"-----BEGIN PRIVATE KEY-----\nc3VwcGxpZWQ=\n-----END PRIVATE KEY-----\n")

    def test_unmatched_certificate_boundary_is_refused(self):
        with self.assertRaisesRegex(worker.AcquisitionError, "PEM structure invalid"):
            worker.public_ca_bytes(b"-----BEGIN CERTIFICATE-----\nc3VwcGxpZWQ=\n")

    def test_oversized_bundle_is_refused(self):
        with self.assertRaisesRegex(worker.AcquisitionError, "oversized"):
            worker.public_ca_bytes(b"x" * (worker.MAX_CA_BYTES + 1))

    def test_non_root_source_is_refused(self):
        with self.assertRaisesRegex(worker.AcquisitionError, "not root-owned"):
            worker.validate_ca_identity(self.identity | {"uid": 501}, source=True)

    def test_group_writable_source_is_refused(self):
        with self.assertRaisesRegex(worker.AcquisitionError, "unsafe or empty"):
            worker.validate_ca_identity(self.identity | {"mode": stat.S_IFREG | 0o664}, source=True)

    def test_world_writable_source_is_refused(self):
        with self.assertRaisesRegex(worker.AcquisitionError, "unsafe or empty"):
            worker.validate_ca_identity(self.identity | {"mode": stat.S_IFREG | 0o646}, source=True)

    def test_nonregular_source_is_refused(self):
        with self.assertRaisesRegex(worker.AcquisitionError, "unsafe or empty"):
            worker.validate_ca_identity(self.identity | {"mode": stat.S_IFIFO | 0o644}, source=True)

    def test_boolean_identity_field_is_refused(self):
        with self.assertRaisesRegex(worker.AcquisitionError, "identity invalid"):
            worker.validate_ca_identity(self.identity | {"uid": False}, source=True)

    def test_changed_snapshot_bytes_cannot_use_old_request(self):
        self.snapshot.chmod(0o600)
        self.snapshot.write_bytes(PEM.replace(b"c3VwcG", b"dGVzdG"))
        with self.assertRaisesRegex(worker.AcquisitionError, "identity/hash differs"):
            self.validate()

    def test_changed_snapshot_mode_is_refused(self):
        self.snapshot.chmod(0o666)
        with self.assertRaisesRegex(worker.AcquisitionError, "unsafe or empty"):
            self.validate()

    def test_bundle_read_uses_original_deadline(self):
        self.clock.advance(10)
        with self.assertRaisesRegex(worker.AcquisitionError, "deadline"):
            self.validate()

    def test_context_has_explicit_cafile_and_unchanged_verification(self):
        self.assertIs(self.context_for(), self.context)
        self.assertEqual(self.factory_calls, [{"cafile": str(self.snapshot)}])
        self.assertEqual(self.context.verify_mode, ssl.CERT_REQUIRED)
        self.assertIs(self.context.check_hostname, True)

    def test_weakened_certificate_verification_is_refused(self):
        self.context.verify_mode = ssl.CERT_NONE
        with self.assertRaisesRegex(worker.AcquisitionError, "verification weakened"):
            self.context_for()

    def test_weakened_hostname_verification_is_refused(self):
        self.context.check_hostname = False
        with self.assertRaisesRegex(worker.AcquisitionError, "verification weakened"):
            self.context_for()

    def test_keylog_context_is_refused(self):
        self.context.keylog_filename = "supplied-secret-path"
        with self.assertRaisesRegex(worker.AcquisitionError, "key logging"):
            self.context_for()

    def test_empty_parsed_ca_context_is_refused(self):
        self.context.cert_store_stats = lambda: {"x509_ca": 0}
        with self.assertRaisesRegex(worker.AcquisitionError, "no trust anchors"):
            self.context_for()

    def test_mock_parse_failure_prevents_context_use(self):
        def malformed(**kwargs):
            raise ssl.SSLError(1, "supplied sensitive parse text")
        with self.assertRaises(ssl.SSLError):
            worker.tls_context(self.request, 10, self.clock, platform="darwin", factory=malformed)

    def test_snapshot_changed_during_context_parse_is_refused(self):
        def changed(**kwargs):
            self.snapshot.chmod(0o600)
            self.snapshot.write_bytes(PEM.replace(b"c3VwcG", b"dGVzdG"))
            return self.context
        with self.assertRaisesRegex(worker.AcquisitionError, "identity/hash differs"):
            worker.tls_context(self.request, 10, self.clock, platform="darwin", factory=changed)

    def test_source_changed_during_context_parse_is_refused(self):
        def changed(**kwargs):
            self.identity["ino"] += 1
            return self.context
        with self.assertRaisesRegex(worker.AcquisitionError, "source identity differs"):
            worker.tls_context(self.request, 10, self.clock, platform="darwin", factory=changed)

    def test_late_context_parse_cannot_authorize_network(self):
        def late(**kwargs):
            self.clock.advance(10)
            return self.context
        with self.assertRaisesRegex(worker.AcquisitionError, "deadline"):
            worker.tls_context(self.request, 10, self.clock, platform="darwin", factory=late)

    def test_partial_ca_read_cannot_renew_deadline_and_closes_handle(self):
        identity = SimpleNamespace(st_dev=1, st_ino=2, st_uid=0, st_gid=0, st_mode=stat.S_IFREG | 0o644,
                                   st_size=len(PEM), st_mtime_ns=3)
        def partial(descriptor, limit):
            self.assertEqual(descriptor, 17)
            self.assertLessEqual(limit, worker.CHUNK)
            self.clock.advance(6)
            return b"x"
        with patch.object(worker.os, "open", return_value=17), patch.object(worker.os, "fstat", return_value=identity), \
             patch.object(worker.os, "read", side_effect=partial) as read, patch.object(worker.os, "close") as close:
            with self.assertRaisesRegex(worker.AcquisitionError, "deadline"):
                worker.bounded_ca_read("/supplied/fake.pem", 10, self.clock)
        self.assertEqual(read.call_count, 2)
        close.assert_called_once_with(17)

    def test_source_change_between_capture_and_freeze_is_refused(self):
        with patch.object(worker, "public_ca_source", side_effect=[(self.resolved, self.identity),
             (self.resolved, self.identity | {"ino": 99})]), \
             patch.object(worker, "bounded_ca_read", return_value=(PEM, self.identity)):
            with self.assertRaisesRegex(worker.AcquisitionError, "source changed"):
                worker.read_public_ca(10, self.clock)

    def test_linux_system_trust_keeps_default_factory(self):
        request = {"tls": {"platform": "linux", "mode": "system_default"}}
        self.assertIs(worker.tls_context(request, 10, self.clock, platform="linux", factory=self.factory), self.context)
        self.assertEqual(self.factory_calls, [{}])

    def test_windows_system_mode_has_no_snapshot_input(self):
        self.assertEqual(parent.acquisition_tls(self.root, self.workspace, 10, self.clock, platform="win32"),
                         {"platform": "win32", "mode": "system_default"})

    def test_non_mac_system_mode_cannot_accept_arbitrary_bundle(self):
        request = {"tls": {"platform": "linux", "mode": "system_default", "snapshot": "/supplied/ca.pem"}}
        with self.assertRaisesRegex(worker.AcquisitionError, "system mode differs"):
            worker.validated_tls(request, 10, self.clock, platform="linux")

    def test_wrong_platform_cannot_reuse_snapshot(self):
        with self.assertRaisesRegex(worker.AcquisitionError, "platform binding differs"):
            worker.validated_tls(self.request, 10, self.clock, platform="linux")

    def test_parent_freezes_exclusive_public_snapshot(self):
        self.snapshot.chmod(0o600)
        self.snapshot.unlink()
        with patch.object(parent, "read_public_ca", return_value=(PEM, self.resolved, self.identity)):
            result = parent.acquisition_tls(self.root, self.workspace, 10, self.clock, platform="darwin")
        self.assertEqual(result["sha256"], hashlib.sha256(PEM).hexdigest())
        self.assertEqual(result["snapshot_identity"], worker.stat_identity(self.snapshot.stat()))
        self.assertEqual(self.snapshot.read_bytes(), PEM)

    def test_parent_never_reuses_snapshot_destination(self):
        with patch.object(parent, "read_public_ca", return_value=(PEM, self.resolved, self.identity)):
            with self.assertRaises(FileExistsError):
                parent.acquisition_tls(self.root, self.workspace, 10, self.clock, platform="darwin")
        self.assertEqual(self.snapshot.read_bytes(), PEM)

    def test_environment_excludes_ssl_and_proxy_overrides(self):
        environment = parent.helper_environment(self.workspace, {"SSL_CERT_FILE": "supplied-foreign",
                    "SSL_CERT_DIR": "supplied-dir", "SSLKEYLOGFILE": "supplied-keylog", "HTTPS_PROXY": "supplied-proxy"},
                    platform="linux")
        self.assertTrue(set(environment).isdisjoint({"SSL_CERT_FILE", "SSL_CERT_DIR", "SSLKEYLOGFILE", "HTTPS_PROXY"}))

    def test_https_handler_receives_exact_verified_context(self):
        body = b"supplied archive"
        metadata = {"tarball": "https://registry.npmjs.org/opaque.tgz", "integrity": "sha512-" +
                    worker.base64.b64encode(hashlib.sha512(body).digest()).decode(), "tarball_sha256": None}
        request = self.request | {"operation_deadline": 10, "metadata": metadata, "archive": str(self.root / "supplied.tgz")}
        opener = FakeOpener(FakeResponse(self.clock, [(body, 0), (b"", 0)]))
        with patch.object(worker, "tls_context", return_value=self.context) as context, \
             patch.object(worker.urllib.request, "HTTPSHandler") as https, \
             patch.object(worker.urllib.request, "build_opener", return_value=opener), \
             patch.object(worker.http.client, "_MAXLINE"), patch.object(worker.http.client, "_MAXHEADERS"):
            worker.download(request, clock=self.clock)
        context.assert_called_once_with(request, 10, self.clock)
        https.assert_called_once_with(context=self.context)


class FailureFactControls(unittest.TestCase):
    def test_url_preserves_nested_ssl_verify_code(self):
        cause = ssl.SSLCertVerificationError(1, "supplied secret cert prose")
        cause.verify_code = 20
        facts = worker.failure_facts(urllib.error.URLError(cause))
        self.assertEqual(facts["type"], "URLError")
        self.assertEqual(facts["reason"], {"type": "SSLCertVerificationError", "category": "ssl_certificate", "verify_code": 20, "errno": 1})
        self.assertNotIn("supplied", json.dumps(facts))

    def test_url_preserves_os_errno_without_paths(self):
        cause = PermissionError(13, "supplied secret prose", "/supplied/secret-path")
        facts = worker.failure_facts(urllib.error.URLError(cause))
        self.assertEqual(facts["reason"], {"type": "PermissionError", "category": "os", "errno": 13})
        self.assertNotIn("secret", json.dumps(facts))

    def test_http_status_is_typed_without_url_headers_body(self):
        cause = urllib.error.HTTPError("https://supplied-secret.invalid", 403, "supplied secret prose",
                                       {"Authorization": "supplied secret header"}, None)
        self.assertEqual(worker.failure_facts(cause), {"type": "HTTPError", "category": "http", "http_status": 403})

    def test_reason_string_is_type_only(self):
        self.assertEqual(worker.failure_facts(urllib.error.URLError("supplied secret text"))["reason"],
                         {"type": "str", "category": "other"})

    def test_unknown_exception_never_calls_repr_or_str(self):
        class SensitiveError(Exception):
            def __str__(self):
                raise AssertionError("No prose serialization")
            def __repr__(self):
                raise AssertionError("No repr serialization")
        self.assertEqual(worker.failure_facts(SensitiveError("supplied-secret")), {"type": "Exception", "category": "other"})

    def test_nested_error_depth_is_bounded(self):
        cause = PermissionError(13, "supplied-secret")
        for _ in range(10):
            cause = urllib.error.URLError(cause)
        facts = worker.failure_facts(cause)
        self.assertEqual(facts["reason"]["reason"]["reason"], {"type": "truncated", "category": "bound"})
        self.assertLess(len(json.dumps(facts)), 512)

    def test_cyclic_url_reason_is_bounded(self):
        cause = urllib.error.URLError("supplied-secret")
        cause.reason = cause
        facts = worker.failure_facts(cause)
        self.assertEqual(facts["reason"], {"type": "truncated", "category": "bound"})

    def test_boolean_verify_code_is_not_numeric_fact(self):
        cause = ssl.SSLCertVerificationError(1, "supplied-secret")
        cause.verify_code = True
        self.assertNotIn("verify_code", worker.failure_facts(cause))

    def test_oversized_errno_is_omitted(self):
        cause = OSError(2 ** 40, "supplied-secret")
        self.assertNotIn("errno", worker.failure_facts(cause))

    def test_gai_error_is_observed_type_not_inferred_dns_prose(self):
        cause = worker.socket.gaierror(-2, "supplied sensitive resolution prose")
        self.assertEqual(worker.failure_facts(urllib.error.URLError(cause))["reason"],
                         {"type": "gaierror", "category": "os", "errno": -2})

    def test_ssl_error_omits_library_reason_and_verify_message(self):
        cause = ssl.SSLError(1, "supplied-secret")
        cause.library = "supplied-secret"
        cause.reason = "supplied-secret"
        self.assertEqual(worker.failure_facts(cause), {"type": "SSLError", "category": "ssl", "errno": 1})

    def test_explicit_exception_cause_is_typed(self):
        cause = worker.AcquisitionError("supplied-secret")
        cause.__cause__ = TimeoutError(110, "supplied-secret")
        self.assertEqual(worker.failure_facts(cause)["cause"], {"type": "TimeoutError", "category": "os", "errno": 110})
