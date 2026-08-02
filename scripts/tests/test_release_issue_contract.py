import json
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "release_issue_contract.py"
FIXTURES = ROOT / "scripts" / "fixtures" / "release-issue-contract"


class ReleaseIssueContractTest(unittest.TestCase):
    def run_contract(self, *args: str) -> subprocess.CompletedProcess:
        return subprocess.run(
            ["python", str(SCRIPT), *args],
            cwd=ROOT,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
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
        for name in ("duplicate.md", "wrong-repo.md", "malformed.md", "prose.md", "closing-keyword.md"):
            with self.subTest(name=name):
                result = self.run_contract("parse", "--body-file", str(FIXTURES / name))
                self.assertNotEqual(result.returncode, 0)

    def test_release_range_unions_only_declared_references(self):
        result = self.run_contract("resolve", "--prs-file", str(FIXTURES / "release-prs.json"))
        self.assertEqual(result.returncode, 0, result.stderr)
        resolved = json.loads(result.stdout)
        self.assertEqual([item["reference"] for item in resolved["issues"]], [
            "rollacode/codeterm-releases#2",
            "rollacode/codeterm-releases#17",
        ])
        self.assertNotIn("#99", result.stdout)

    def test_close_plan_is_idempotent_and_has_no_mutation_command(self):
        resolved = json.loads(self.run_contract("resolve", "--prs-file", str(FIXTURES / "release-prs.json")).stdout)
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
        resolve = workflow.index("- name: Resolve included private PR public issues")
        close = workflow.index("- name: Close shipped public issues")
        self.assertLess(publish, verify)
        self.assertLess(verify, resolve)
        self.assertLess(resolve, close)
        close_step = workflow[close:]
        self.assertNotIn("if: always()", close_step)
        self.assertNotIn("continue-on-error", close_step)
        self.assertIn("--repo rollacode/codeterm-releases", close_step)
        self.assertNotIn("gh issue close \"$number\" \\\n+              --repo \"$GITHUB_REPOSITORY\"", close_step)


if __name__ == "__main__":
    unittest.main()
