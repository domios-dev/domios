import importlib.util
import json
import sys
import unittest
from argparse import Namespace
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
SPEC = importlib.util.spec_from_file_location(
    "rerank_live_runner", Path(__file__).with_name("run.py")
)
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)


class RequestModeTests(unittest.TestCase):
    def test_old_build_default_never_reads_or_writes_request_mode(self):
        args = Namespace(dev_instance="fixture", request_mode=None)
        expected = (Path("results"), Path("summary"), "opaque-summary")
        with patch.object(runner, "run_evaluation", return_value=expected) as evaluate:
            with patch.object(runner, "run_dc_command") as command:
                self.assertEqual(runner.run(args), expected)
        evaluate.assert_called_once_with(args)
        command.assert_not_called()

    def test_selection_restores_previous_mode_after_success_and_failure(self):
        args = Namespace(dev_instance="fixture", request_mode="shared_state")
        for fail in (False, True):
            with self.subTest(fail=fail):
                with patch.object(
                    runner, "run_dc_command", return_value=json.dumps({"value": "per_candidate"})
                ) as command:
                    with patch.object(runner, "run_evaluation") as evaluate:
                        if fail:
                            evaluate.side_effect = RuntimeError("opaque-failure")
                            with self.assertRaisesRegex(RuntimeError, "opaque-failure"):
                                runner.run(args)
                        else:
                            evaluate.return_value = "opaque-result"
                            self.assertEqual(runner.run(args), "opaque-result")
                calls = [call.args[2] for call in command.call_args_list]
                key = "supporter.memory_rerank.request_mode"
                self.assertEqual(calls, [
                    ["config", "get", key, "--json"],
                    ["config", "set", key, "shared_state", "--json"],
                    ["config", "set", key, "per_candidate", "--json"],
                ])

    def test_missing_capability_fails_before_any_write_or_evaluation(self):
        args = Namespace(dev_instance="fixture", request_mode="shared_state")
        with patch.object(runner, "run_dc_command", return_value="{}") as command:
            with patch.object(runner, "run_evaluation") as evaluate:
                with self.assertRaises(RuntimeError):
                    runner.run(args)
        self.assertEqual(command.call_count, 1)
        evaluate.assert_not_called()


if __name__ == "__main__":
    unittest.main()
