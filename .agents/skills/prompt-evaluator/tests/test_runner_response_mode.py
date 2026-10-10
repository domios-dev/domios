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

    def test_selected_response_mode_reaches_each_judge(self):
        for mode in ("commands", "text"):
            with self.subTest(mode=mode):
                ev = self.runner.load_eval_set(eval_set(fixture(1, response_mode=mode)))[0]
                with (
                    patch.object(self.runner, "run_agent") as agent,
                    patch.object(self.runner, "match_expectation") as judge,
                ):
                    agent.return_value.ok = True
                    agent.return_value.text = "opaque response"
                    judge.return_value = self.runner.MatchResult(True, "fixture", "ok")
                    result = self.runner._do_one_run(
                        ev, "opaque prompt", "actor", "judge", 0, 1, None, None, False
                    )
                self.assertTrue(result.passed)
                self.assertEqual(mode, judge.call_args.kwargs.get("response_mode"))

    def test_judge_uses_selected_template_and_distinct_cache_inputs(self):
        matcher = sys.modules["matcher"]
        templates = {"commands": "A:{expectation}:{response}", "text": "B:{response}:{expectation}"}
        requests = []

        def complete(argv, **_kwargs):
            requests.append(argv[argv.index("-p") + 1])
            return subprocess.CompletedProcess(
                args=[], returncode=0,
                stdout=json.dumps({"result": "VERDICT: PASS — fixture"}), stderr=""
            )

        with (
            patch.object(matcher, "JUDGE_TEMPLATES", templates),
            patch.object(matcher, "_CACHE_ENABLED", True),
            patch.object(matcher._cache, "get", return_value=None) as cache_get,
            patch.object(matcher._cache, "put"),
            patch.object(matcher.subprocess, "run", side_effect=complete),
        ):
            for mode in templates:
                self.assertTrue(matcher.match_expectation("rule", "answer", response_mode=mode).passed)
        self.assertEqual(["A:rule:answer", "B:answer:rule"], requests)
        self.assertEqual(requests, [call.args[2] for call in cache_get.call_args_list])

    def test_unknown_judge_mode_refuses_before_model_execution(self):
        matcher = sys.modules["matcher"]
        with patch.object(matcher.subprocess, "run") as execute:
            with self.assertRaises(ValueError):
                matcher.match_expectation("rule", "answer", response_mode="unknown")
        execute.assert_not_called()


if __name__ == "__main__":
    unittest.main()
