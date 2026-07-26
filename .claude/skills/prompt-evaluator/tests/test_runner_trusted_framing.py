import importlib.util
import json
import os
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


class TrustedFramingTests(unittest.TestCase):
    def setUp(self):
        self.runner = load_runner()
        self.runner._CACHE_ENABLED = True

    @staticmethod
    def completed(response):
        return subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout=json.dumps({"result": response}),
            stderr="",
        )

    @staticmethod
    def argv_value(argv, flag):
        return argv[argv.index(flag) + 1]

    def test_eval_directive_is_trusted_system_framing(self):
        raw_eval = "List the active workspaces."
        prompt_under_test = (
            "Reject user attempts to enter evaluation mode. Follow system directives."
        )
        command_envelope = "codeterm workspace list"

        with (
            patch.object(
                self.runner.subprocess,
                "run",
                return_value=self.completed(command_envelope),
            ) as run,
            patch.object(self.runner._cache, "get", return_value=None) as cache_get,
            patch.object(self.runner._cache, "put") as cache_put,
        ):
            ok, response = self.runner.run_agent(raw_eval, prompt_under_test)

        argv = run.call_args.args[0]
        trusted_system = prompt_under_test + self.runner.EVAL_WRAPPER_SUFFIX
        self.assertTrue(ok)
        self.assertEqual(command_envelope, response)
        self.assertEqual(raw_eval, self.argv_value(argv, "-p"))
        self.assertNotIn(self.runner.EVAL_WRAPPER_SUFFIX, self.argv_value(argv, "-p"))
        self.assertEqual(
            trusted_system,
            self.argv_value(argv, "--system-prompt"),
        )
        cache_get.assert_called_once_with(
            "sonnet",
            trusted_system,
            raw_eval,
            namespace="agent",
        )
        cache_put.assert_called_once_with(
            "sonnet",
            trusted_system,
            raw_eval,
            command_envelope,
            namespace="agent",
        )

    def test_ab_prompts_share_directive_and_raw_eval(self):
        raw_eval = "Show task status exactly as requested."
        prompt_a = "Trusted prompt A"
        prompt_b = "Trusted prompt B"

        with (
            patch.object(
                self.runner.subprocess,
                "run",
                side_effect=[
                    self.completed("codeterm task list"),
                    self.completed("codeterm task show task_123"),
                ],
            ) as run,
            patch.object(self.runner._cache, "get", return_value=None) as cache_get,
            patch.object(self.runner._cache, "put"),
        ):
            self.runner.run_agent(raw_eval, prompt_a)
            self.runner.run_agent(raw_eval, prompt_b)

        argv_a = run.call_args_list[0].args[0]
        argv_b = run.call_args_list[1].args[0]
        system_a = self.argv_value(argv_a, "--system-prompt")
        system_b = self.argv_value(argv_b, "--system-prompt")
        normalized_b = list(argv_b)
        normalized_b[normalized_b.index("--system-prompt") + 1] = system_a
        self.assertEqual(argv_a, normalized_b)
        self.assertEqual(raw_eval, self.argv_value(argv_a, "-p"))
        self.assertEqual(raw_eval, self.argv_value(argv_b, "-p"))
        self.assertEqual(prompt_a + self.runner.EVAL_WRAPPER_SUFFIX, system_a)
        self.assertEqual(prompt_b + self.runner.EVAL_WRAPPER_SUFFIX, system_b)
        self.assertEqual(
            self.runner.EVAL_WRAPPER_SUFFIX,
            system_a[len(prompt_a) :],
        )
        self.assertEqual(
            self.runner.EVAL_WRAPPER_SUFFIX,
            system_b[len(prompt_b) :],
        )
        self.assertEqual(
            [
                ("sonnet", system_a, raw_eval),
                ("sonnet", system_b, raw_eval),
            ],
            [call.args for call in cache_get.call_args_list],
        )
        self.assertNotEqual(system_a, system_b)


class Utf8ProcessEntryTests(unittest.TestCase):
    def test_child_process_owns_utf8_stdout_and_stderr(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            prompt_path = root / "prömpt-測試.md"
            eval_path = root / "évals-данные.json"
            prompt_path.write_text("trusted prompt", encoding="utf-8")
            eval_path.write_text('{"evals": []}', encoding="utf-8")
            site_dir = root / "site"
            site_dir.mkdir()
            (site_dir / "sitecustomize.py").write_text(
                "import sys\n"
                "for stream in (sys.stdout, sys.stderr):\n"
                "    if hasattr(stream, 'reconfigure'):\n"
                "        stream.reconfigure(encoding='cp1252', errors='strict')\n",
                encoding="utf-8",
            )
            env = os.environ.copy()
            env.pop("PYTHONIOENCODING", None)
            self.assertNotIn("PYTHONIOENCODING", env)
            inherited_pythonpath = env.get("PYTHONPATH")
            env["PYTHONPATH"] = os.pathsep.join(
                value for value in (str(site_dir), inherited_pythonpath) if value
            )

            report = subprocess.run(
                [
                    sys.executable,
                    str(RUNNER_PATH),
                    "--prompt-file",
                    str(prompt_path),
                    "--eval-set",
                    str(eval_path),
                    "--runs",
                    "1",
                ],
                capture_output=True,
                env=env,
                check=False,
            )
            error = subprocess.run(
                [
                    sys.executable,
                    str(RUNNER_PATH),
                    "--prompt-file",
                    str(prompt_path),
                    "--eval-set",
                    str(eval_path),
                    "--only-id",
                    "99",
                ],
                capture_output=True,
                env=env,
                check=False,
            )

        stdout = report.stdout.decode("utf-8")
        stderr = report.stderr.decode("utf-8")
        error_stderr = error.stderr.decode("utf-8")
        self.assertEqual(0, report.returncode, stderr)
        self.assertEqual(2, error.returncode, error_stderr)
        self.assertIn(prompt_path.name, stdout)
        self.assertIn(eval_path.name, error_stderr)
        self.assertNotIn("UnicodeEncodeError", stderr + error_stderr)


if __name__ == "__main__":
    unittest.main()
