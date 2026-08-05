import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "release_issue_contract.py"
FIXTURES = ROOT / "scripts" / "fixtures" / "release-issue-contract"


class ReleaseIssueContractTest(unittest.TestCase):
    def run_contract(self, *args: str) -> subprocess.CompletedProcess:
        return subprocess.run(
            [sys.executable, str(SCRIPT), *args],
            cwd=ROOT,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )

    def resolve_fixture(self, prs: str = "release-prs.json", commits: str = "release-commits.json"):
        return self.run_contract(
            "resolve",
            "--commits-file",
            str(FIXTURES / commits),
            "--prs-file",
            str(FIXTURES / prs),
        )

    def test_valid_and_zero_references_normalize(self):
        valid = self.run_contract("parse", "--body-file", str(FIXTURES / "valid-pr.md"))
        self.assertEqual(valid.returncode, 0, valid.stderr)
        self.assertEqual(valid.stdout.splitlines(), [
            "rollacode/codeterm-releases#2",
            "rollacode/codeterm-releases#17",
        ])
        empty = self.run_contract("parse", "--body-file", str(FIXTURES / "zero-refs.md"))
        self.assertEqual(empty.returncode, 0, empty.stderr)
        self.assertEqual(empty.stdout, "")

    def test_invalid_fixtures_are_rejected(self):
        for name in (
            "duplicate.md",
            "wrong-repo.md",
            "malformed.md",
            "prose.md",
            "closing-keyword.md",
            "url-prose.md",
            "closing-keyword-url.md",
        ):
            with self.subTest(name=name):
                result = self.run_contract("parse", "--body-file", str(FIXTURES / name))
                self.assertNotEqual(result.returncode, 0)

    def test_release_range_unions_only_declared_references(self):
        result = self.resolve_fixture()
        self.assertEqual(result.returncode, 0, result.stderr)
        resolved = json.loads(result.stdout)
        self.assertEqual([item["reference"] for item in resolved["issues"]], [
            "rollacode/codeterm-releases#2",
            "rollacode/codeterm-releases#17",
            "rollacode/codeterm-releases#32",
        ])
        self.assertNotIn("#99", result.stdout)
        issue_17 = next(issue for issue in resolved["issues"] if issue["reference"].endswith("#17"))
        self.assertEqual([source["kind"] for source in issue_17["sources"]], [
            "pull_request",
            "commit",
        ])

    def test_commit_rejects_malformed_metadata_but_ignores_incidental_issue_prose(self):
        result = self.resolve_fixture(commits="malformed-release-commits.json")
        self.assertEqual(result.returncode, 2)
        self.assertIn("commit 4444444444444444444444444444444444444444", result.stderr)
        self.assertIn("malformed Public-Issue metadata", result.stderr)

    def test_failed_tag_does_not_advance_successful_release_range(self):
        start = self.run_contract(
            "range-start",
            "--releases-file",
            str(FIXTURES / "failed-then-successful-releases.json"),
        )
        self.assertEqual(start.returncode, 0, start.stderr)
        self.assertEqual(start.stdout.strip(), "1111111111111111111111111111111111111111")
        resolved_result = self.run_contract(
            "resolve",
            "--commits-file",
            str(FIXTURES / "empty-commits.json"),
            "--prs-file",
            str(FIXTURES / "failed-then-successful-prs.json"),
        )
        self.assertEqual(resolved_result.returncode, 0, resolved_result.stderr)
        resolved = json.loads(resolved_result.stdout)
        self.assertEqual(
            [issue["reference"] for issue in resolved["issues"]],
            ["rollacode/codeterm-releases#31", "rollacode/codeterm-releases#32"],
        )
        with tempfile.TemporaryDirectory() as directory:
            resolved_path = Path(directory) / "resolved.json"
            states_path = Path(directory) / "states.json"
            resolved_path.write_text(json.dumps(resolved), encoding="utf-8")
            states_path.write_text(json.dumps({
                "rollacode/codeterm-releases#31": "OPEN",
                "rollacode/codeterm-releases#32": "OPEN",
            }), encoding="utf-8")
            first = self.run_contract(
                "plan-close",
                "--resolved-file",
                str(resolved_path),
                "--states-file",
                str(states_path),
                "--version",
                "1.8.2",
            )
            self.assertEqual([action["number"] for action in json.loads(first.stdout)], [31, 32])
            states_path.write_text(json.dumps({
                "rollacode/codeterm-releases#31": "CLOSED",
                "rollacode/codeterm-releases#32": "CLOSED",
            }), encoding="utf-8")
            second = self.run_contract(
                "plan-close",
                "--resolved-file",
                str(resolved_path),
                "--states-file",
                str(states_path),
                "--version",
                "1.8.2",
            )
            self.assertEqual(json.loads(second.stdout), [])

    def test_close_plan_is_idempotent_and_has_no_mutation_command(self):
        resolved = json.loads(self.resolve_fixture().stdout)
        with tempfile.TemporaryDirectory() as directory:
            resolved_path = Path(directory) / "resolved.json"
            states_path = Path(directory) / "states.json"
            resolved_path.write_text(json.dumps(resolved), encoding="utf-8")
            states_path.write_text(json.dumps({
                "rollacode/codeterm-releases#2": "OPEN",
                "rollacode/codeterm-releases#17": "CLOSED",
            }), encoding="utf-8")
            first = self.run_contract("plan-close", "--resolved-file", str(resolved_path), "--states-file", str(states_path), "--version", "1.8.0")
            actions = json.loads(first.stdout)
            self.assertEqual([action["number"] for action in actions], [2])
            self.assertEqual(actions[0]["comment"], "Released in v1.8.0\n\nhttps://github.com/rollacode/codeterm/pull/10")
            self.assertNotIn("gh issue", first.stdout)
            states_path.write_text(json.dumps({key: "CLOSED" for key in (
                "rollacode/codeterm-releases#2", "rollacode/codeterm-releases#17"
            )}), encoding="utf-8")
            second = self.run_contract("plan-close", "--resolved-file", str(resolved_path), "--states-file", str(states_path), "--version", "1.8.0")
            self.assertEqual(json.loads(second.stdout), [])

    def test_issue_mutation_is_after_verified_publication(self):
        workflow = (ROOT / ".github" / "workflows" / "release.yml").read_text(encoding="utf-8")
        publish = workflow.index("- name: Publish (private archive + public mirror + signed latest.json)")
        verify = workflow.index("- name: Verify published public mirror")
        resolve = workflow.index("- name: Resolve included private source public issues")
        close = workflow.index("- name: Close shipped public issues")
        self.assertLess(publish, verify)
        self.assertLess(verify, resolve)
        self.assertLess(resolve, close)
        close_step = workflow[close:]
        self.assertNotIn("if: always()", close_step)
        self.assertNotIn("continue-on-error", close_step)
        self.assertIn("--repo rollacode/codeterm-releases", close_step)
        self.assertNotIn("gh issue close \"$number\" \\\n+              --repo \"$GITHUB_REPOSITORY\"", close_step)

    def test_pr_validation_includes_commit_metadata(self):
        workflow = (ROOT / ".github" / "workflows" / "ready-for-release.yml").read_text(encoding="utf-8")
        self.assertIn("git rev-list", workflow)
        self.assertIn('--commits-file "$RUNNER_TEMP/release-commits.json"', workflow)
        self.assertIn('--prs-file "$RUNNER_TEMP/release-prs.json"', workflow)


if __name__ == "__main__":
    unittest.main()
