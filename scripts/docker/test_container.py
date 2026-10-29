import importlib.util
import contextlib
import io
import json
import pathlib
import subprocess
import unittest
from unittest import mock
import tempfile
from types import SimpleNamespace

spec = importlib.util.spec_from_file_location(
    "container_supervisor", pathlib.Path(__file__).parents[2] / "packaging/docker/container.py"
)
subject = importlib.util.module_from_spec(spec)
spec.loader.exec_module(subject)
signal = SimpleNamespace(SIGTERM=15, SIGINT=2, SIGKILL=9)
subject.signal = signal


class InjectedProcess:
    def __init__(self, events, during_spawn=False):
        self.events = iter(events)
        self.during_spawn = during_spawn
        self.handler = None
        self.live = False
        self.signals = []
        self.deadlines = []
        self.reaped = False
        self.closed = False
        self.failures = []

    def install(self, handler):
        self.handler = handler

    def wake(self):
        pass

    def start(self):
        assert self.handler is not None
        if self.during_spawn:
            self.handler(signal.SIGTERM)
        self.live = True

    def forward(self, signum):
        assert self.live
        self.signals.append(signum)
        if signum == signal.SIGKILL:
            self.live = False

    def wait(self, deadline):
        self.deadlines.append(deadline)
        event = next(self.events)
        if event == "stop":
            self.handler(signal.SIGTERM)
            return "signal"
        if event == "stop-and-exit":
            self.handler(signal.SIGTERM)
            event = "exit"
        if event == "exit":
            self.live = False
        return event

    def reap(self, timeout):
        assert timeout in (0, 2, 20)
        if self.live:
            raise subprocess.TimeoutExpired("owned-daemon", timeout)
        self.reaped = True

    def close(self):
        self.closed = True

    def diagnostics(self, reason):
        self.failures.append(reason)


class SupervisorTests(unittest.TestCase):
    def supervise(self, process):
        return subject.Supervisor(process, clock=lambda: 10).run(lambda: None)

    def test_signal_while_startup_readiness_is_held_is_forwarded_and_reaped(self):
        process = InjectedProcess(["stop", "exit"])
        self.assertEqual(self.supervise(process), 0)
        self.assertEqual(process.signals, [signal.SIGTERM])
        self.assertEqual(process.deadlines, [130, 30])
        self.assertFalse(process.live)
        self.assertTrue(process.reaped)
        self.assertTrue(process.closed)

    def test_signal_in_spawn_window_is_queued_for_the_exact_child(self):
        process = InjectedProcess(["exit"], during_spawn=True)
        self.assertEqual(self.supervise(process), 0)
        self.assertEqual(process.signals, [signal.SIGTERM])
        self.assertEqual(process.deadlines, [30])
        self.assertTrue(process.reaped)
        self.assertFalse(process.live)

    def test_ready_state_retains_signal_forwarding(self):
        process = InjectedProcess(["ready", "stop", "exit"])
        self.assertEqual(self.supervise(process), 0)
        self.assertEqual(process.deadlines, [130, None, 30])
        self.assertEqual(process.signals, [signal.SIGTERM])
        self.assertTrue(process.reaped)

    def test_stalled_cleanup_is_killed_and_reaped_with_a_fixed_bound(self):
        process = InjectedProcess(["stop", "timeout"])
        self.assertEqual(self.supervise(process), 1)
        self.assertEqual(process.signals, [signal.SIGTERM, signal.SIGKILL])
        self.assertEqual(process.deadlines, [130, 30])
        self.assertFalse(process.live)
        self.assertTrue(process.reaped)

    def test_startup_timeout_cleans_the_owned_child(self):
        process = InjectedProcess(["timeout", "exit"])
        self.assertEqual(self.supervise(process), 1)
        self.assertEqual(process.signals, [signal.SIGTERM])
        self.assertTrue(process.reaped)

    def test_signal_and_exit_in_the_same_event_batch_exit_cleanly(self):
        process = InjectedProcess(["stop-and-exit"])
        self.assertEqual(self.supervise(process), 0)
        self.assertTrue(process.reaped)
        self.assertFalse(process.live)

    def test_signal_before_spawn_starts_no_daemon(self):
        process = InjectedProcess([])
        supervisor = subject.Supervisor(process, clock=lambda: 10)
        self.assertEqual(supervisor.run(lambda: process.handler(signal.SIGTERM)), 0)
        self.assertFalse(process.live)
        self.assertTrue(process.closed)

    def test_unexpected_exit_emits_diagnostics_once(self):
        process = InjectedProcess(["ready", "exit"])
        self.assertEqual(self.supervise(process), 1)
        self.assertEqual(process.failures, ["unexpected exit"])

    def test_normal_stop_emits_no_failure_diagnostics(self):
        process = InjectedProcess(["ready", "stop", "exit"])
        self.assertEqual(self.supervise(process), 0)
        self.assertEqual(process.failures, [])


class OfflineMemoryTests(unittest.TestCase):
    def test_migration_delegates_to_existing_offline_server_and_preserves_failure(self):
        with mock.patch.object(subject, "prepare") as prepare, mock.patch.object(subject.subprocess, "run") as run:
            run.return_value.returncode = 7
            self.assertEqual(subject.command(["migrate-memory", "--to", "postgres"]), 7)
            prepare.assert_called_once_with()
            run.assert_called_once_with([
                subject.SERVER, "memory-migrate", "--to", "postgres", "--json",
            ], check=False)

    def test_ambiguous_migration_arguments_start_no_process(self):
        with mock.patch.object(subject.subprocess, "run") as run:
            for args in [["migrate-memory"], ["migrate-memory", "--to", "remote"],
                         ["migrate-memory", "--to", "postgres", "--into-nonempty"]]:
                with self.assertRaises(ValueError):
                    subject.command(args)
            run.assert_not_called()

    def test_offline_selection_preserves_config_and_leaves_no_temporary_file(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            config = root / "config.toml"
            config.write_text('[api]\nhost = "0.0.0.0"\n[memory]\nbackend = "sqlite"\nother = 9\n[memory.embedder]\nprovider = "local"\n')
            with mock.patch.object(subject, "DATA", root), mock.patch.object(subject, "prepare"), contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(subject.command(["select-memory", "--to", "postgres"]), 0)
            self.assertEqual(subject.tomllib.loads(config.read_text()), {
                "api": {"host": "0.0.0.0"},
                "memory": {"backend": "postgres", "other": 9, "embedder": {"provider": "local"}},
            })
            self.assertEqual(list(root.iterdir()), [config])

    def test_selection_write_failure_preserves_previous_configuration(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            config = root / "config.toml"
            original = '[memory]\nbackend = "sqlite"\n'
            config.write_text(original)
            with mock.patch.object(subject, "DATA", root), mock.patch.object(subject, "prepare"), mock.patch.object(subject.os, "replace", side_effect=OSError("fixture")):
                with self.assertRaises(OSError):
                    subject.select_memory("postgres")
            self.assertEqual(config.read_text(), original)
            self.assertEqual(list(root.iterdir()), [config])


class DiagnosticsTests(unittest.TestCase):
    def test_failed_startup_log_is_bounded_and_redacts_local_and_url_credentials(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / "logs").mkdir()
            old = "previous startup must not be forwarded\n"
            log = root / "logs/domios.log"
            log.write_text(old + "x" * 70000 + '\ncurrent memory failure Bearer standalone\n'
                           'postgresql://domios:password-fragment@postgres/db api_token=readiness-token\n'
                           'password-fragment local-value\n')
            (root / ".secrets").write_text(json.dumps({
                "domios.instance:api_token": "readiness-token",
                "local": json.dumps({"value": "local-value"}),
                "database": json.dumps({"value": "postgresql://domios:password-fragment@postgres/db"}),
            }))
            runtime = subject.ProcessRuntime.__new__(subject.ProcessRuntime)
            runtime.log_offset = len(old)
            stderr = io.StringIO()
            with mock.patch.object(subject, "DATA", root), mock.patch.object(subject, "private"), contextlib.redirect_stderr(stderr):
                runtime.diagnostics("unexpected exit")
            output = stderr.getvalue()
            self.assertIn("current memory failure", output)
            self.assertIn("[REDACTED]", output)
            self.assertLess(len(output), 8500)
            for secret in ["readiness-token", "local-value", "password-fragment", "standalone", "previous startup"]:
                self.assertNotIn(secret, output)

    def test_unreadable_secret_store_withholds_log_contents(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / "logs").mkdir()
            (root / "logs/domios.log").write_text("sensitive fixture\n")
            (root / ".secrets").write_text("not valid JSON")
            runtime = subject.ProcessRuntime.__new__(subject.ProcessRuntime)
            runtime.log_offset = 0
            stderr = io.StringIO()
            with mock.patch.object(subject, "DATA", root), mock.patch.object(subject, "private"), contextlib.redirect_stderr(stderr):
                runtime.diagnostics("unexpected exit")
            self.assertIn("unavailable", stderr.getvalue())
            self.assertNotIn("sensitive fixture", stderr.getvalue())


if __name__ == "__main__":
    unittest.main()
