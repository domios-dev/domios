"""Sampling-integrity tests: `--runs N` must mean N distinct samples.

The defect these guard against: the response cache was keyed on content only,
so `--runs 3` made one agent call and one judge call, then reported that single
verdict three times behind a confidence interval claiming n=3.

Every assertion here is a mechanism assertion — call counts, cache keys,
distinct-sample counts, and values the test itself supplied round-tripping into
the rendered summary. No judge prose, no model output.
"""

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

AGENT_FLAG = "--system-prompt-file"
CONSTANT_AGENT_RESPONSE = "codeterm workspace create demo"
CONSTANT_JUDGE_VERDICT = "VERDICT: PASS - satisfied"


def load_skill_modules(cache_dir: Path):
    """Load runner + matcher + cache bound to an isolated on-disk cache dir."""
    os.environ["CODETERM_EVAL_CACHE_DIR"] = str(cache_dir)
    for name in ("cache", "matcher", "prompt_evaluator_runner"):
        sys.modules.pop(name, None)
    if str(SKILL_DIR) not in sys.path:
        sys.path.insert(0, str(SKILL_DIR))
    spec = importlib.util.spec_from_file_location("prompt_evaluator_runner", RUNNER_PATH)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def completed(response):
    return subprocess.CompletedProcess(
        args=[], returncode=0, stdout=json.dumps({"result": response}), stderr=""
    )


class _CallRecorder:
    """Stub agent + judge that always answer identically.

    Identical answers are the point: if repeated samples still produce repeated
    calls, only the per-sample cache key can be separating them.
    """

    def __init__(self):
        self.agent_calls = []
        self.judge_calls = []

    def __call__(self, argv, **_kwargs):
        if AGENT_FLAG in argv:
            self.agent_calls.append(list(argv))
            return completed(CONSTANT_AGENT_RESPONSE)
        self.judge_calls.append(list(argv))
        return completed(CONSTANT_JUDGE_VERDICT)

    @staticmethod
    def payload(argv):
        return argv[argv.index("-p") + 1]

    def agent_payloads(self):
        return {self.payload(a) for a in self.agent_calls}

    def judge_payloads(self):
        return {self.payload(a) for a in self.judge_calls}


class SampleIntegrityTestCase(unittest.TestCase):
    def setUp(self):
        self._prior_cache_dir = os.environ.get("CODETERM_EVAL_CACHE_DIR")
        self._tmp = tempfile.TemporaryDirectory()
        self.cache_dir = Path(self._tmp.name) / "cache"
        self.runner = load_skill_modules(self.cache_dir)
        self.cache = self.runner._cache
        self.matcher = sys.modules["matcher"]

    def tearDown(self):
        if self._prior_cache_dir is None:
            os.environ.pop("CODETERM_EVAL_CACHE_DIR", None)
        else:
            os.environ["CODETERM_EVAL_CACHE_DIR"] = self._prior_cache_dir
        for name in ("cache", "matcher", "prompt_evaluator_runner"):
            sys.modules.pop(name, None)
        self._tmp.cleanup()

    def make_eval(self, expectations=("uses workspace create",)):
        return self.runner.Eval(
            id=7,
            name="sample-integrity",
            prompt="Create a workspace for the demo.",
            expected_output="a workspace create command",
            expectations=list(expectations),
        )

    def execute(self, ev, runs, recorder=None):
        recorder = recorder or _CallRecorder()
        with patch.object(subprocess, "run", side_effect=recorder):
            results = self.runner.run_eval_n_times(
                ev,
                system_prompt="prompt under test",
                runs=runs,
                main_model="sonnet",
                judge_model="haiku",
                verbose=False,
            )
        return recorder, results

    def cache_entries(self):
        return sorted(p.name for p in self.cache_dir.glob("*.json"))


class RunsMeanSamplesTests(SampleIntegrityTestCase):
    def test_runs_3_makes_three_agent_calls_and_three_judge_calls(self):
        recorder, results = self.execute(self.make_eval(), runs=3)

        self.assertEqual(3, len(recorder.agent_calls))
        self.assertEqual(3, len(recorder.judge_calls))
        # Identical payloads prove the per-sample key, not differing content,
        # is what kept these calls from collapsing into one cached entry.
        self.assertEqual(1, len(recorder.agent_payloads()))
        self.assertEqual(1, len(recorder.judge_payloads()))
        self.assertEqual(3, len(results))
        self.assertEqual(3, len({r.sample_key for r in results}))

    def test_each_sample_writes_its_own_cache_entry(self):
        self.execute(self.make_eval(), runs=3)

        entries = self.cache_entries()
        self.assertEqual(6, len(entries))          # 3 agent + 3 judge
        self.assertEqual(len(set(entries)), len(entries))

    def test_sample_salt_gives_every_run_index_a_distinct_key(self):
        keys = {
            self.cache.key(
                "sonnet",
                "system",
                "user",
                namespace="agent",
                salt=self.cache.sample_salt(i),
            )
            for i in range(5)
        }
        self.assertEqual(5, len(keys))

    def test_second_invocation_replays_distinct_samples_without_new_calls(self):
        self.execute(self.make_eval(), runs=3)
        recorder, results = self.execute(self.make_eval(), runs=3)

        self.assertEqual([], recorder.agent_calls)
        self.assertEqual([], recorder.judge_calls)
        self.assertEqual(3, len({r.sample_key for r in results}))
        self.assertTrue(all(r.sample_from_cache for r in results))

        summary = self.runner.summarize(self.make_eval(), results, requested_runs=3)
        self.assertEqual(3, summary.total_runs)
        self.assertFalse(summary.undersampled)


class CacheProvenanceLabelTests(SampleIntegrityTestCase):
    def test_cache_served_result_is_labelled_in_the_summary(self):
        ev = self.make_eval()
        self.execute(ev, runs=2)
        recorder, results = self.execute(ev, runs=2)

        self.assertEqual([], recorder.agent_calls)
        summary = self.runner.summarize(ev, results, requested_runs=2)
        self.assertEqual(2, summary.cached_samples)
        self.assertEqual(2, summary.cached_judgements)
        self.assertEqual("cached", summary.source_label)

        report = self.runner.render_single_report("prompt.md", [summary])
        self.assertIn(summary.source_label, report)
        for line in self.runner.render_sampling_note([summary]):
            if line:
                self.assertIn(line, report)

    def test_runs_1_with_warm_cache_still_serves_from_cache_and_says_so(self):
        ev = self.make_eval()
        first_recorder, first_results = self.execute(ev, runs=1)
        self.assertEqual(1, len(first_recorder.agent_calls))
        fresh = self.runner.summarize(ev, first_results, requested_runs=1)
        self.assertEqual("live", fresh.source_label)

        recorder, results = self.execute(ev, runs=1)
        self.assertEqual([], recorder.agent_calls)
        self.assertEqual([], recorder.judge_calls)

        warm = self.runner.summarize(ev, results, requested_runs=1)
        self.assertEqual(1, warm.total_runs)
        self.assertEqual("cached", warm.source_label)
        self.assertIn(warm.source_label, self.runner.render_single_report("p.md", [warm]))

    def test_warm_and_fresh_sampling_notes_are_distinguishable(self):
        ev = self.make_eval()
        _, fresh_results = self.execute(ev, runs=1)
        _, warm_results = self.execute(ev, runs=1)

        fresh_note = self.runner.render_sampling_note(
            [self.runner.summarize(ev, fresh_results, requested_runs=1)]
        )
        warm_note = self.runner.render_sampling_note(
            [self.runner.summarize(ev, warm_results, requested_runs=1)]
        )

        self.assertNotEqual(fresh_note, warm_note)
        for note in (fresh_note, warm_note):
            self.assertTrue(any(self.runner.SAMPLING_NOTE_MARKER in line for line in note))


class ReportedSampleSizeTests(SampleIntegrityTestCase):
    def duplicated_runs(self, count, sample_key="collapsed"):
        return [
            self.runner.RunResult(
                eval_id=7,
                eval_name="sample-integrity",
                run_idx=i,
                response=CONSTANT_AGENT_RESPONSE,
                sample_key=sample_key,
                sample_from_cache=True,
                matches=[self.matcher.MatchResult(True, "llm-judge", "ok", from_cache=True)],
            )
            for i in range(count)
        ]

    def test_repeated_sample_keys_collapse_to_one_reported_sample(self):
        summary = self.runner.summarize(
            self.make_eval(), self.duplicated_runs(3), requested_runs=3
        )

        self.assertEqual(1, summary.total_runs)
        self.assertEqual(1, summary.runs_passed)
        self.assertEqual("1/1", summary.pass_rate)
        self.assertEqual(3, summary.requested_runs)
        self.assertTrue(summary.undersampled)

    def test_confidence_interval_uses_distinct_samples_not_requested_runs(self):
        summary = self.runner.summarize(
            self.make_eval(), self.duplicated_runs(3), requested_runs=3
        )

        reported = self.runner.wilson_ci(summary.runs_passed, summary.total_runs)
        one_sample = self.runner.wilson_ci(1, 1)
        three_samples = self.runner.wilson_ci(3, 3)

        self.assertEqual(one_sample, reported)
        self.assertNotEqual(three_samples, reported)
        self.assertIn(self.runner.fmt_ci(*one_sample), self.runner.render_single_report("p.md", [summary]))

    def test_undersampled_run_set_is_flagged_in_the_report(self):
        undersampled = self.runner.summarize(
            self.make_eval(), self.duplicated_runs(3), requested_runs=3
        )
        honest = self.runner.summarize(
            self.make_eval(), self.duplicated_runs(1), requested_runs=1
        )

        self.assertNotEqual(
            self.runner.render_sampling_note([undersampled]),
            self.runner.render_sampling_note([honest]),
        )
        report = self.runner.render_single_report("p.md", [undersampled])
        for line in self.runner.render_sampling_note([undersampled]):
            if line:
                self.assertIn(line, report)


class CacheFlagsStillWorkTests(SampleIntegrityTestCase):
    def test_no_cache_takes_fresh_calls_every_invocation(self):
        self.runner._CACHE_ENABLED = False
        self.matcher.set_cache_enabled(False)
        ev = self.make_eval()

        first, first_results = self.execute(ev, runs=2)
        second, _ = self.execute(ev, runs=2)

        self.assertEqual(2, len(first.agent_calls))
        self.assertEqual(2, len(second.agent_calls))
        self.assertEqual(2, len(first.judge_calls))
        self.assertEqual(2, len(second.judge_calls))
        self.assertEqual([], self.cache_entries())

        summary = self.runner.summarize(ev, first_results, requested_runs=2)
        self.assertEqual(2, summary.total_runs)
        self.assertEqual(0, summary.cached_samples)
        self.assertEqual("live", summary.source_label)

    def test_clear_cache_wipes_every_entry(self):
        self.execute(self.make_eval(), runs=2)
        self.assertEqual(4, len(self.cache_entries()))

        removed = self.cache.nuke()

        self.assertEqual(4, removed)
        self.assertEqual([], self.cache_entries())

        recorder, _ = self.execute(self.make_eval(), runs=2)
        self.assertEqual(2, len(recorder.agent_calls))


if __name__ == "__main__":
    unittest.main()
