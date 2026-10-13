import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SHELL = os.environ.get("BENCH_TEST_BASH", "/bin/bash")


def shell(body):
    return subprocess.run([SHELL, "-u", "-o", "pipefail", "-c", body],
                          cwd=ROOT.parent, text=True, capture_output=True)


def function(script, start, end):
    text = (ROOT / script).read_text()
    return text[text.index(start):text.index(end, text.index(start))]


class BenchInfrastructure(unittest.TestCase):
    def test_empty_and_incomplete_refused_and_exit_preserved(self):
        guard = f'source "{ROOT}/_lib/bench.sh"\n'
        for body, status in [("bench_complete", 1), ("bench_assertion", 1),
                             ("bench_assertion; bench_complete", 0),
                             ("cleanup() { return 0; }; exit 23", 23),
                             ("bench_assertion\ninvalid { syntax", 1)]:
            with self.subTest(body=body):
                result = shell(guard + body)
                if "invalid" in body:
                    self.assertNotEqual(result.returncode, 0, result.stderr)
                else:
                    self.assertEqual(result.returncode, status, result.stderr)

    def test_portable_stream_retains_fast_exit_output_and_failure(self):
        result = shell(f'source "{ROOT}/_lib/bench.sh"\n'
                       'bench_stream_start bash -c \'printf "READY\\nRESULT\\n"; exit 23\'\n'
                       'worker=$BENCH_STREAM_PID\nread -r first <&8\nread -r second <&8\n'
                       '[ "$first" = READY ] && [ "$second" = RESULT ] || exit 1\n'
                       'wait "$worker"')
        self.assertEqual(result.returncode, 23, result.stderr)

    def test_empty_restore_workers(self):
        body = function("restore-matrix/run.sh", "send_all_markers() {", "assert_post_chat_isolation() {")
        with tempfile.TemporaryDirectory() as tmp:
            tsv = Path(tmp) / "slots.tsv"
            tsv.write_text("claude\t1\tmodel\t\tmarker\t\t\t1\tskipped\t\n")
            result = shell(f'SLOTS_TSV="{tsv}"\nmerge_slots() {{ :; }}\n' + body + '\nsend_all_markers')
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_fleet_rejects_empty_missing_unbound_and_duplicate(self):
        with tempfile.TemporaryDirectory() as tmp:
            track, tabs = Path(tmp) / "tracked", Path(tmp) / "tabs.json"
            for tracked, rows, expected in [
                ("", [], 1),
                ("a claude ws /tmp\n", [], 1),
                ("a claude ws /tmp\n", [{"id": "a"}], 1),
                ("a claude ws /tmp\nb codex ws /tmp\n",
                 [{"id": "a", "agent_session_id": "same"}, {"id": "b", "agent_session_id": "same"}], 1),
                ("a claude ws /tmp\n", [{"id": "a", "agent_session_id": "own"}], 0),
            ]:
                track.write_text(tracked)
                tabs.write_text(json.dumps(rows))
                result = subprocess.run(["python3", str(ROOT / "binding-matrix/check_fleet.py"),
                                         str(track), str(tabs), "fixture"], capture_output=True)
                self.assertEqual(result.returncode, expected, result.stderr)

    def test_portable_array_loading(self):
        body = function("binding-matrix/run.sh", "CLAUDE_TABS=()", "if [ \"$CLAUDE_COUNT\" -gt 0 ]; then")
        with tempfile.TemporaryDirectory() as tmp:
            track = Path(tmp) / "tracked"
            for rows, count in [("", 0), ("a claude ws cwd\nb codex ws cwd\nc claude ws cwd\n", 2)]:
                track.write_text(rows)
                result = shell(f'TRACK="{track}"\n' + body + f'\n[ "$CLAUDE_COUNT" -eq {count} ]')
                self.assertEqual(result.returncode, 0, result.stderr)

    def test_real_gate_release_after_server_exit(self):
        body = function("long-paste-submit/run.sh", "release_gate() {", "cleanup() {")
        gate = ROOT / "long-paste-submit/gate.py"
        result = shell(f'source "{ROOT}/_lib/bench.sh"\nBENCH="{gate.parent}"; EVENT_TIMEOUT=5\n' + body +
                       f'\nbench_stream_start python3 "{gate}" server 5\nGATE_PID=$BENCH_STREAM_PID\n'
                       'read -r GATE_PORT <&8\n'
                       f'python3 "{gate}" client "$GATE_PORT" 5 >/dev/null &\nclient=$!\n'
                       'read -r event <&8\n[ "$event" = CONNECTED ] || exit 1\n'
                       'GATE_ACTIVE=1; GATE_CONNECTED=1\nrelease_gate\nwait "$client"\n'
                       '[ "$GATE_ACTIVE" -eq 0 ]; bench_assertion; bench_complete')
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_current_ledger_state(self):
        body = function("chat-roundtrip/run.sh", "assert_delivered_state() {", "count_user_turns() {")
        for state, expected in [("delivered", 0), ("confirmed", 1), ("held", 1),
                                ("submit_unconfirmed", 1), ("failed", 1)]:
            result = shell('pass() { :; }; fail() { exit 1; }\n' + body +
                           f'\nassert_delivered_state fixture case "{state}"')
            self.assertEqual(result.returncode, expected, result.stderr)

    def test_long_paste_oracle(self):
        spec = importlib.util.spec_from_file_location("outcome", ROOT / "long-paste-submit/assert_outcome.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        messages = [{"type": "user", "content": "opaque payload"},
                    {"type": "assistant", "content": "opaque reply"}]
        ledger = [{"send_id": "own", "state": "delivered"}]
        frames = [b"\x1b[200~opaque payload\x1b[201~"]
        sends = [("own", "opaque payload")]
        module.assert_outcome(messages, ledger, frames, sends)
        for args in [(messages, ledger, frames, []), ([], ledger, frames, sends),
                     (messages * 2, ledger, frames, sends), (messages, [], frames, sends),
                     (messages, [{"send_id": "own", "state": "confirmed"}], frames, sends),
                     (messages[:1], ledger, frames, sends), (messages, ledger, frames * 2, sends)]:
            with self.subTest(args=args), self.assertRaises(AssertionError):
                module.assert_outcome(*args)

    def test_native_bash_syntax(self):
        for name in ["_lib/bench.sh", "_lib/role_preset.sh", "binding-matrix/run.sh",
                     "long-paste-submit/run.sh", "lifecycle-recovery/run.sh",
                     "restore-matrix/run.sh", "chat-roundtrip/run.sh", "chat-roundtrip/test-exit-status.sh", "chat-roundtrip/test-picker-trigger.sh"]:
            result = subprocess.run([SHELL, "-n", str(ROOT / name)], capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
