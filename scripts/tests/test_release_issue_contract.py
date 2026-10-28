# Caller: scripts/tests/release-issue-resolution.test.mjs
import copy
import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from release_issue_contract import BOUNDARY_VERIFICATION_STEPS, closure_boundary_refusal
from release_issue_resolution import included_history, included_pull_requests, MAX_PR_PAGES


class ClosureBoundaryTests(unittest.TestCase):
    def setUp(self):
        self.sha = "a" * 40
        self.run = {"id": 50, "run_attempt": 3, "conclusion": "cancelled",
                    "path": ".github/workflows/release.yml"}
        self.job = {"run_id": 50, "run_attempt": 3, "status": "completed",
                    "steps": [{"number": number, "name": name, "conclusion": "success"}
                              for number, name in enumerate(BOUNDARY_VERIFICATION_STEPS, 16)]
                    + [{"number": 19, "name": "Resolve included private source public issues",
                        "conclusion": "cancelled"}]}
        self.release = {"sha": self.sha, "closure_boundary": {
            "source_sha": self.sha, "public_mirror_verified": True, "validator_run_id": 50},
            "validator_run": self.run, "validator_jobs": [self.job]}

    def test_cancelled_later_issue_scan_keeps_verified_boundary(self):
        self.assertIsNone(closure_boundary_refusal(self.release))

    def test_failed_later_issue_scan_keeps_verified_boundary(self):
        self.run["conclusion"] = "failure"
        self.job["steps"][3]["conclusion"] = "failure"
        self.assertIsNone(closure_boundary_refusal(self.release))

    def test_failed_or_skipped_mint_refuses(self):
        for conclusion in ("failure", "skipped"):
            with self.subTest(conclusion=conclusion):
                self.job["steps"][2]["conclusion"] = conclusion
                self.assertIsNotNone(closure_boundary_refusal(self.release))

    def test_foreign_run_or_attempt_refuses(self):
        for field, value in (("run_id", 51), ("run_attempt", 2)):
            with self.subTest(field=field):
                release = copy.deepcopy(self.release)
                release["validator_jobs"][0][field] = value
                self.assertIsNotNone(closure_boundary_refusal(release))
        self.run["id"] = 51
        self.assertIsNotNone(closure_boundary_refusal(self.release))

    def test_full_success_needs_no_job_fallback(self):
        self.run["conclusion"] = "success"
        self.release.pop("validator_jobs")
        self.assertIsNone(closure_boundary_refusal(self.release))

    def test_verification_missing_failed_or_split_across_jobs_refuses(self):
        for index in range(3):
            for conclusion in ("failure", "skipped", None):
                with self.subTest(index=index, conclusion=conclusion):
                    release = copy.deepcopy(self.release)
                    release["validator_jobs"][0]["steps"][index]["conclusion"] = conclusion
                    self.assertIsNotNone(closure_boundary_refusal(release))
        release = copy.deepcopy(self.release)
        release["validator_jobs"][0]["steps"] = self.job["steps"][:2]
        release["validator_jobs"].append({**self.job, "steps": self.job["steps"][2:]})
        self.assertIsNotNone(closure_boundary_refusal(release))

    def test_failure_before_boundary_refuses_even_if_required_steps_succeeded(self):
        self.job["steps"].insert(0, {"number": 15, "name": "Earlier operation", "conclusion": "failure"})
        self.assertIsNotNone(closure_boundary_refusal(self.release))

    def test_step_names_and_order_are_required(self):
        self.job["steps"][0]["name"] = "Unrelated verification"
        self.assertIsNotNone(closure_boundary_refusal(self.release))
        self.job["steps"][0]["name"] = BOUNDARY_VERIFICATION_STEPS[0]
        self.job["steps"][0]["number"] = 20
        self.assertIsNotNone(closure_boundary_refusal(self.release))

    def test_existing_identity_checks_still_refuse(self):
        for section, field, value in (("closure_boundary", "source_sha", "b" * 40),
                                      ("closure_boundary", "public_mirror_verified", False),
                                      ("validator_run", "path", ".github/workflows/tests.yml")):
            with self.subTest(field=field):
                release = copy.deepcopy(self.release)
                release[section][field] = value
                self.assertIsNotNone(closure_boundary_refusal(release))

    def test_resolver_fetches_jobs_for_exact_attempt(self):
        from release_issue_contract import PUBLIC_REPOSITORY, SOURCE_REPOSITORY
        prefix = f"repos/{SOURCE_REPOSITORY}"
        published = {"draft": False, "published_at": "2026-10-27", "assets": [
            {"id": 11, "name": "issue-closure-boundary.json"}, {"name": "domios-latest.json"}]}
        routes = {
            f"{prefix}/releases/tags/v1.16.0": published,
            f"repos/{PUBLIC_REPOSITORY}/releases/tags/v1.16.0": published,
            f"{prefix}/releases/assets/11": json.dumps(self.release["closure_boundary"]),
            f"{prefix}/actions/runs/50": self.run,
            f"{prefix}/actions/runs/50/attempts/3/jobs?per_page=100&page=1": {"jobs": [self.job]},
        }
        calls = []

        def request(route, raw=False):
            calls.append(route)
            return routes[route]

        def commits(start, source, limit):
            self.assertEqual(start, self.sha)
            return []

        result = included_history("1.17.0", "b" * 40,
                                  [{"version": "1.16.0", "sha": self.sha}], commits, request)
        self.assertEqual(result["start"], self.sha)
        self.assertEqual(calls[-1], f"{prefix}/actions/runs/50/attempts/3/jobs?per_page=100&page=1")


class ClosedPullHistoryTests(unittest.TestCase):
    def setUp(self):
        self.sha = "a" * 40
        self.calls = []

    def pr(self, number, **changes):
        return {"number": number, "html_url": f"https://example.invalid/pull/{number}",
                "body": f"Repository-Issue: domios-dev/domios#{number}",
                "merged_at": "2026-10-27T12:00:00Z", "updated_at": "2026-10-27T12:00:00Z",
                "merge_commit_sha": self.sha, "base": {"ref": "next"}, **changes}

    def request(self, pages):
        def fetch(route):
            self.assertIn("/pulls?state=closed&sort=updated&direction=desc&per_page=100&page=", route)
            self.calls.append(route)
            return pages[int(route.rsplit("=", 1)[1]) - 1]
        return fetch

    def test_pages_keep_only_merged_included_prs_and_reach_boundary_margin(self):
        pages = [[self.pr(1), self.pr(2, merge_commit_sha="b" * 40), self.pr(3, merged_at=None)]
                 + [self.pr(4, body="No issue metadata")] * 97,
                 [self.pr(5)] * 100,
                 [self.pr(6, merged_at="2026-10-24T23:59:59Z", updated_at="2026-10-24T23:59:59Z")] * 100]
        result = included_pull_requests({self.sha}, self.sha, "2026-10-27T00:00:00Z", self.request(pages))
        self.assertEqual([pr["number"] for pr in result], [1, 5, 6])
        self.assertEqual(len(self.calls), 3)

    def test_page_bound_before_boundary_refuses(self):
        with self.assertRaisesRegex(ValueError, "page bound before boundary date"):
            included_pull_requests({self.sha}, self.sha, "2026-10-27T00:00:00Z",
                                   self.request([[self.pr(1)] * 100] * MAX_PR_PAGES))
        self.assertEqual(len(self.calls), MAX_PR_PAGES)

    def test_source_pr_closing_keywords_remain_included(self):
        result = included_pull_requests({self.sha}, self.sha, "2026-10-27T00:00:00Z",
                                       self.request([[self.pr(1, body="Closes #1")]]))
        self.assertEqual([pr["number"] for pr in result], [1])


if __name__ == "__main__":
    unittest.main()
