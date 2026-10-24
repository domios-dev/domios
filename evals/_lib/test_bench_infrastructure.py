import importlib.util
import json
import os
import shlex
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

    def test_split_dev_target_pins_server_to_selected_provenance(self):
        body = function("_lib/lib.sh", "dev_assert_target() {", "dev_assert_cached_identity() {")
        sha = "a" * 40
        with tempfile.TemporaryDirectory() as tmp:
            data = Path(tmp)
            record = data / "dev-build-provenance.json"
            for name, port in [("", "7686"), ("lane", "7786")]:
                health = {"status": "ok", "startup": {"state": "ready"}, "build_sha": sha,
                          "instance": {"name": name, "api_port": port}}
                provenance = {"schema": 3, "buildSha": sha, "instance": {
                    "name": name, "apiPort": port, "vitePort": "1420", "cdpPort": "9223"}}

                def check(value, receipt):
                    record.write_text(json.dumps(receipt))
                    script = (body + "\n"
                              + "dev_api_url() { printf http://fixture.invalid; }\n"
                              + "dev_resolution_error() { :; }\n"
                              + "curl() { printf %s " + shlex.quote(json.dumps(value)) + "; }\n"
                              + "EVAL_LIB_TARGET_INSTANCE=" + shlex.quote(name) + "\n"
                              + "EVAL_LIB_TARGET_API_PORT=" + port + "\n"
                              + "EVAL_LIB_TARGET_VITE_PORT=1420 EVAL_LIB_TARGET_CDP_PORT=9223\n"
                              + "EVAL_LIB_TARGET_DATA_DIR=" + shlex.quote(data.as_posix()) + "\n"
                              + "EVAL_LIB_RESOLVED_DATA_DIR=\n"
                              + "dev_assert_target")
                    return shell(script)

                with self.subTest(instance=name):
                    self.assertEqual(check(health, provenance).returncode, 0)
                    for bad in [dict(health, status="starting"), dict(health, build_sha="b" * 40),
                                dict(health, build_sha=sha[:8]),
                                dict(health, instance={"name": "foreign", "api_port": port}),
                                dict(health, instance={"name": name, "api_port": "7685"})]:
                        self.assertEqual(check(bad, provenance).returncode, 7)
                    wrong = dict(provenance, instance=dict(provenance["instance"], apiPort="7685"))
                    self.assertEqual(check(health, wrong).returncode, 7)
                    legacy = dict(health, instance=dict(health["instance"], vite_port="1420", cdp_port="9223"))
                    self.assertEqual(check(legacy, provenance).returncode, 0)
                    wrong_gui = dict(health, instance=dict(legacy["instance"], cdp_port="9999"))
                    self.assertEqual(check(wrong_gui, provenance).returncode, 7)

    def test_named_dev_uses_provenance_ports_and_retains_identity_guard(self):
        resolver = function("_lib/lib.sh", "dev_provenance_ports() {", "dev_resolve_identity() {")
        guard = function("_lib/lib.sh", "dev_assert_target() {", "dev_assert_cached_identity() {")
        sha = "c" * 40
        with tempfile.TemporaryDirectory() as tmp:
            data = Path(tmp)
            record = data / "dev-build-provenance.json"
            receipt = {"schema": 3, "buildSha": sha, "instance": {
                "name": "roglaunch", "apiPort": "17696", "vitePort": "14996", "cdpPort": "9296"}}
            health = {"status": "ok", "startup": {"state": "ready"}, "build_sha": sha,
                      "instance": {"name": "roglaunch", "api_port": "17696"}}

            def check(value, live):
                record.write_text(json.dumps(value))
                script = (resolver + guard + "\nset -e\n"
                          + "dev_api_url() { printf http://fixture.invalid; }\n"
                          + "dev_resolution_error() { :; }\n"
                          + "curl() { printf %s " + shlex.quote(json.dumps(live)) + "; }\n"
                          + "ports=$(dev_provenance_ports " + shlex.quote(record.as_posix()) + " roglaunch 17696)\n"
                          + "IFS=$'\\t' read -r EVAL_LIB_TARGET_VITE_PORT EVAL_LIB_TARGET_CDP_PORT <<<\"$ports\"\n"
                          + "EVAL_LIB_TARGET_INSTANCE=roglaunch EVAL_LIB_TARGET_API_PORT=17696\n"
                          + "EVAL_LIB_TARGET_DATA_DIR=" + shlex.quote(data.as_posix()) + "\n"
                          + "EVAL_LIB_RESOLVED_DATA_DIR=\n"
                          + "dev_assert_target\nprintf '%s %s' \"$EVAL_LIB_TARGET_VITE_PORT\" \"$EVAL_LIB_TARGET_CDP_PORT\"")
                return shell(script)

            good = check(receipt, health)
            self.assertEqual(good.returncode, 0, good.stderr)
            self.assertEqual(good.stdout, "14996 9296")
            self.assertEqual(check(receipt, dict(health, build_sha="d" * 40)).returncode, 7)
            for field, value in [("name", "foreign"), ("apiPort", "7685"),
                                 ("vitePort", "7685"), ("cdpPort", "17696"),
                                 ("cdpPort", "99999"), ("vitePort", "invalid")]:
                with self.subTest(field=field, value=value):
                    bad = dict(receipt, instance=dict(receipt["instance"], **{field: value}))
                    self.assertEqual(check(bad, health).returncode, 7)
            for bad in [dict(receipt, schema=2), dict(receipt, buildSha=sha[:8])]:
                self.assertEqual(check(bad, health).returncode, 7)

    def test_native_bash_syntax(self):
        for name in ["_lib/bench.sh", "_lib/role_preset.sh", "binding-matrix/run.sh",
                     "long-paste-submit/run.sh", "lifecycle-recovery/run.sh",
                     "restore-matrix/run.sh", "chat-roundtrip/run.sh", "chat-roundtrip/test-exit-status.sh", "chat-roundtrip/test-picker-trigger.sh"]:
            result = subprocess.run([SHELL, "-n", str(ROOT / name)], capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
