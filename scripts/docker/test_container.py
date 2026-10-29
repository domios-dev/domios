import importlib.util
import pathlib
import subprocess
import unittest
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


if __name__ == "__main__":
    unittest.main()
