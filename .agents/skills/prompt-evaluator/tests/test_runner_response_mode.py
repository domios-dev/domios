import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


SKILL_DIR = Path(__file__).resolve().parents[1]
RUNNER_PATH = SKILL_DIR / "runner.py"


def load_runner():
    spec = importlib.util.spec_from_file_location("prompt_evaluator_runner", RUNNER_PATH)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def eval_set(*items):
    handle = tempfile.NamedTemporaryFile(
        "w", suffix=".json", delete=False, encoding="utf-8"
    )
    with handle as fh:
        json.dump({"evals": list(items)}, fh)
    return handle.name


def fixture(eval_id, **extra):
    item = {
        "id": eval_id,
        "name": f"case-{eval_id}",
        "prompt": "scenario",
        "expectations": ["something"],
    }
    item.update(extra)
    return item


class ResponseModeTests(unittest.TestCase):
    def setUp(self):
        self.runner = load_runner()

    def test_omitting_the_mode_keeps_the_command_envelope(self):
        loaded = self.runner.load_eval_set(eval_set(fixture(1)))
        self.assertEqual("commands", loaded[0].response_mode)
        self.assertEqual(
            self.runner.EVAL_WRAPPER_SUFFIX,
            self.runner.WRAPPERS[loaded[0].response_mode],
        )

    def test_text_mode_selects_the_prose_envelope(self):
        loaded = self.runner.load_eval_set(eval_set(fixture(1, response_mode="text")))
        self.assertEqual(
            self.runner.EVAL_WRAPPER_SUFFIX_TEXT,
            self.runner.WRAPPERS[loaded[0].response_mode],
        )

    def test_the_two_envelopes_are_distinct(self):
        self.assertNotEqual(
            self.runner.EVAL_WRAPPER_SUFFIX,
            self.runner.EVAL_WRAPPER_SUFFIX_TEXT,
        )

    def test_an_unknown_mode_is_refused_at_load(self):
        with self.assertRaises(ValueError) as caught:
            self.runner.load_eval_set(eval_set(fixture(7, response_mode="freeform")))
        self.assertIn("freeform", str(caught.exception))

    def test_the_selected_envelope_reaches_the_spawned_system_prompt(self):
        spawned = []

        def complete(argv, **_kwargs):
            path = Path(argv[argv.index("--system-prompt-file") + 1])
            spawned.append(path.read_text(encoding="utf-8"))
            return subprocess.CompletedProcess(
                args=[], returncode=0, stdout=json.dumps({"result": "ok"}), stderr=""
            )

        with (
            patch.object(self.runner.subprocess, "run", side_effect=complete),
            patch.object(self.runner._cache, "get", return_value=None),
            patch.object(self.runner._cache, "put"),
        ):
            self.runner.run_agent(
                "scenario", "PROMPT", wrapper=self.runner.EVAL_WRAPPER_SUFFIX_TEXT
            )

        self.assertEqual("PROMPT" + self.runner.EVAL_WRAPPER_SUFFIX_TEXT, spawned[0])

    def test_the_envelope_salts_the_cache_key(self):
        commands = self.runner._cache.key(
            "sonnet",
            "PROMPT" + self.runner.EVAL_WRAPPER_SUFFIX,
            "scenario",
            namespace="agent",
            salt=self.runner._cache.sample_salt(0),
        )
        text = self.runner._cache.key(
            "sonnet",
            "PROMPT" + self.runner.EVAL_WRAPPER_SUFFIX_TEXT,
            "scenario",
            namespace="agent",
            salt=self.runner._cache.sample_salt(0),
        )
        self.assertNotEqual(commands, text)


if __name__ == "__main__":
    unittest.main()
