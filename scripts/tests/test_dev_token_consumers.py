import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import types
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]


def load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


connection = load("dev_connection_fixture", "evals/_lib/dev_connection.py")
SELECTED = {
    "CODETERM_API_URL": "http://127.0.0.1:7791",
    "CODETERM_API_PORT": "7791",
    "CODETERM_API_TOKEN": "selected-dev",
    "CODETERM_SOCKET_PATH": "domios-dev-fixture.sock",
    "CODETERM_DATA_DIR": "/fixture/.domios-dev-fixture",
    "CODETERM_TAB_ID": "",
}


class DevTokenConsumers(unittest.TestCase):
    def test_adapter_uses_library_environment_and_discards_inherited_authority(self):
        inherited = {
            "CODETERM_API_TOKEN": "prod",
            "CODETERM_API_URL": "http://127.0.0.1:7685",
            "CODETERM_API_PORT": "7685",
            "CODETERM_TASK_ID": "prod-task",
            "CODETERM_DEV_INSTANCE": "fixture",
            "CODETERM_DEV_API_PORT": "7791",
        }
        completed = subprocess.CompletedProcess([], 0, json.dumps(SELECTED), "")
        with patch.dict(os.environ, inherited), patch.object(connection.subprocess, "run", return_value=completed) as run:
            resolved = connection.dev_environment()
        self.assertEqual(connection.dev_token(resolved), "selected-dev")
        self.assertEqual(connection.dev_api_url(resolved), SELECTED["CODETERM_API_URL"])
        self.assertNotIn("CODETERM_TASK_ID", resolved)
        self.assertEqual(run.call_args.kwargs["env"]["CODETERM_INSTANCE"], "fixture")
        self.assertEqual(run.call_args.kwargs["env"]["CODETERM_API_PORT"], "7791")
        self.assertIn('source "$1"; dev_run', run.call_args.args[0][2])

    def test_adapter_failure_has_no_environment_token_fallback(self):
        with patch.object(connection.subprocess, "run", side_effect=subprocess.CalledProcessError(7, "fixture")):
            with self.assertRaises(subprocess.CalledProcessError):
                connection.dev_token()

    def fake_library(self):
        return types.SimpleNamespace(
            dev_environment=lambda: dict(SELECTED),
            dev_token=connection.dev_token,
            dev_api_url=connection.dev_api_url,
        )

    def test_replay_capture_requests_use_the_selected_connection(self):
        with patch.dict(sys.modules, {"dev_connection": self.fake_library()}):
            replay = load("replay_capture_fixture", "testbench/replay-capture.py")
        with patch.object(replay.urllib.request, "urlopen", return_value=io.BytesIO(b"{}")) as opened:
            replay.dev_get("/api/tabs")
        request = opened.call_args.args[0]
        self.assertEqual(request.full_url, SELECTED["CODETERM_API_URL"] + "/api/tabs")
        self.assertEqual(request.get_header("Authorization"), "Bearer selected-dev")
        self.assertEqual(replay.ENV["CODETERM_SOCKET_PATH"], SELECTED["CODETERM_SOCKET_PATH"])

    def test_chat_timeline_resolves_once_and_passes_the_selected_connection(self):
        with patch.dict(sys.modules, {"dev_connection": self.fake_library()}):
            timeline = load("chat_timeline_fixture", "testbench/streaming/record-chat-timeline.py")
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "timeline.jsonl"
            with patch.object(sys, "argv", ["timeline", "--tab", "fixture", "--out", str(output)]), patch.object(timeline, "poll_chat") as poll:
                self.assertEqual(timeline.main(), 0)
            self.assertEqual(poll.call_args.args[1], "selected-dev")
            self.assertEqual(poll.call_args.args[-1], SELECTED["CODETERM_API_URL"])
            self.assertEqual(json.loads(output.read_text())["kind"], "start")
        with patch.object(timeline.urllib.request, "urlopen", return_value=io.BytesIO(b"{}")) as opened:
            timeline.request("/api/tabs", "selected-dev", 5, SELECTED["CODETERM_API_URL"])
        self.assertEqual(opened.call_args.args[0].full_url, SELECTED["CODETERM_API_URL"] + "/api/tabs")
        self.assertEqual(opened.call_args.args[0].get_header("Authorization"), "Bearer selected-dev")


if __name__ == "__main__":
    unittest.main()
