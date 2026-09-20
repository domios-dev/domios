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
        closes = self.run_contract(
            "repository-closes",
            "--body-file",
            str(FIXTURES / "valid-pr.md"),
        )
        self.assertEqual(closes.returncode, 0, closes.stderr)
        self.assertEqual(closes.stdout.splitlines(), ["Closes #40", "Closes #294"])
        empty = self.run_contract("parse", "--body-file", str(FIXTURES / "zero-refs.md"))
        self.assertEqual(empty.returncode, 0, empty.stderr)
        self.assertEqual(empty.stdout, "")

    def test_invalid_fixtures_are_rejected(self):
        for name in (
            "duplicate.md",
            "trailing-prose-ref.md",
            "wrong-repo.md",
            "malformed.md",
            "malformed-repository.md",
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
        self.assertEqual([item["reference"] for item in resolved["repository_issues"]], [
            "rollacode/codeterm#40",
            "rollacode/codeterm#294",
        ])
        self.assertNotIn("#99", result.stdout)
        issue_17 = next(issue for issue in resolved["issues"] if issue["reference"].endswith("#17"))
        self.assertEqual([source["kind"] for source in issue_17["sources"]], [
            "pull_request",
            "commit",
        ])

    def test_repository_closes_are_accepted_only_for_master_prs(self):
        with tempfile.TemporaryDirectory() as directory:
            commits_path = Path(directory) / "commits.json"
            prs_path = Path(directory) / "prs.json"
            commits_path.write_text("[]", encoding="utf-8")
            master_pr = [{
                "number": 293,
                "url": "https://github.com/rollacode/codeterm/pull/293",
                "body": "Closes #40\nCloses #294",
                "base_ref": "master",
            }]
            prs_path.write_text(json.dumps(master_pr), encoding="utf-8")
            accepted = self.run_contract(
                "resolve",
                "--commits-file",
                str(commits_path),
                "--prs-file",
                str(prs_path),
            )
            self.assertEqual(accepted.returncode, 0, accepted.stderr)
            self.assertEqual(
                [item["reference"] for item in json.loads(accepted.stdout)["repository_issues"]],
                ["rollacode/codeterm#40", "rollacode/codeterm#294"],
            )
            master_pr[0]["base_ref"] = "develop"
            prs_path.write_text(json.dumps(master_pr), encoding="utf-8")
            rejected = self.run_contract(
                "resolve",
                "--commits-file",
                str(commits_path),
                "--prs-file",
                str(prs_path),
            )
            self.assertEqual(rejected.returncode, 2)
            self.assertIn("GitHub closing keywords are forbidden", rejected.stderr)

    def test_commit_rejects_malformed_metadata_but_ignores_incidental_issue_prose(self):
        result = self.resolve_fixture(commits="malformed-release-commits.json")
        self.assertEqual(result.returncode, 2)
        self.assertIn("commit 4444444444444444444444444444444444444444", result.stderr)
        self.assertIn("malformed Public-Issue metadata", result.stderr)

    def test_trailing_declaration_closes_a_prose_line(self):
        """A commit message cannot be edited, so a declaration that ends a prose
        line has to resolve rather than fail every range that contains it."""
        result = self.resolve_fixture(
            prs="empty-commits.json",
            commits="trailing-declaration-commits.json",
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        resolved = json.loads(result.stdout)
        self.assertEqual(
            [issue["reference"] for issue in resolved["repository_issues"]],
            ["rollacode/codeterm#308"],
        )
        self.assertEqual(
            [issue["reference"] for issue in resolved["issues"]],
            ["rollacode/codeterm-releases#42"],
        )

        accepted = self.run_contract(
            "parse", "--body-file", str(FIXTURES / "trailing-declaration.md")
        )
        self.assertEqual(accepted.returncode, 0, accepted.stderr)

    def test_prose_before_a_trailing_declaration_stays_under_contract(self):
        rejected = self.run_contract(
            "parse", "--body-file", str(FIXTURES / "trailing-prose-ref.md")
        )
        self.assertEqual(rejected.returncode, 2)
        self.assertIn("issue references must use issue metadata", rejected.stderr)

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

    def test_publishing_workflow_mutates_no_issue(self):
        """The caller delegates issue closure to its final manifest job."""
        workflow = (ROOT / ".github" / "workflows" / "release.yml").read_text(encoding="utf-8")
        self.assertIn("macos-linux-arm64:", workflow)
        self.assertIn("linux-x86_64:", workflow)
        self.assertIn("windows:", workflow)
        self.assertIn("release-manifest:", workflow)
        self.assertIn("uses: ./.github/workflows/release-manifest-validate.yml", workflow)
        self.assertNotIn("gh issue close", workflow)
        self.assertNotIn("- name: Close shipped public issues", workflow)

    def test_issue_mutation_is_after_verified_publication(self):
        workflow = (ROOT / ".github" / "workflows" / "release-manifest-validate.yml").read_text(encoding="utf-8")
        merge = workflow.index("- name: Merge and upload final channel manifests")
        mark = workflow.index("- name: Mark source and public releases published")
        verify = workflow.index("- name: Verify published public mirror")
        boundary = workflow.index("- name: Mint issue closure boundary")
        resolve = workflow.index("- name: Resolve included private source public issues")
        close = workflow.index("- name: Close shipped public issues")
        self.assertLess(merge, mark)
        self.assertLess(mark, verify)
        self.assertLess(verify, boundary)
        self.assertLess(boundary, resolve)
        self.assertLess(resolve, close)
        for start in (verify, boundary, resolve, close):
            step = workflow[start:workflow.index("shell: bash", start)]
            self.assertIn("if: inputs.mirror", step)
        close_step = workflow[close:]
        self.assertNotIn("if: always()", close_step)
        self.assertNotIn("continue-on-error", close_step)
        self.assertIn('--repo "$MIRROR_REPO"', close_step)
        self.assertNotIn('--repo "$GITHUB_REPOSITORY"', close_step)

    def test_release_pr_collection_is_paginated_not_per_commit(self):
        workflow = (ROOT / ".github" / "workflows" / "release-manifest-validate.yml").read_text(encoding="utf-8")
        resolve = workflow[workflow.index("- name: Resolve included private source public issues") :]
        self.assertIn("gh api --paginate", resolve)
        self.assertIn("jq -s", resolve)
        self.assertNotIn("--paginate --slurp", resolve)
        self.assertIn("pulls?state=closed", resolve)
        self.assertIn("$included[.merge_commit_sha]", resolve)
        self.assertIn('--arg source_sha "$SOURCE_SHA"', resolve)
        self.assertIn('test("(?im)^(?:Public-Issue:|Repository-Issue:)"', resolve)
        self.assertIn(".merge_commit_sha == $source_sha", resolve)
        self.assertNotIn('commits/$sha/pulls', resolve)

    def test_pr_validation_includes_commit_metadata(self):
        workflow = (ROOT / ".github" / "workflows" / "ready-for-release.yml").read_text(encoding="utf-8")
        self.assertIn("git rev-list", workflow)
        self.assertIn('--commits-file "$RUNNER_TEMP/release-commits.json"', workflow)
        self.assertIn('--prs-file "$RUNNER_TEMP/release-prs.json"', workflow)
        self.assertIn("--arg base_ref '${{ github.event.pull_request.base.ref }}'", workflow)
        self.assertIn(".repository_issues[].reference", workflow)
        self.assertIn("--repo rollacode/codeterm", workflow)



class Release1108ClosureContractTest(unittest.TestCase):
    """The 1.10.8 milestone integrates on release/1.10.8, so closing keywords are
    inert until the release PR reaches master. The Closes block in that PR is
    rendered, never typed: a hand-edited list is an unchecked second source."""

    FIXED = (370, 368, 343, 305, 371, 372, 373, 321, 374, 318)

    def run_contract(self, *args: str) -> subprocess.CompletedProcess:
        return subprocess.run(
            [sys.executable, str(SCRIPT), *args],
            cwd=ROOT,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )

    def expected_closes(self) -> list[str]:
        return [f"Closes #{number}" for number in sorted(self.FIXED)]

    def test_release_body_renders_exactly_the_fixed_set(self):
        result = self.run_contract(
            "repository-closes", "--body-file", str(FIXTURES / "release-1108-pr.md")
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines(), self.expected_closes())
        # #336 is carried forward, not fixed. A closing keyword for it would be a
        # false promise on a public issue, which is worse than no reference.
        self.assertNotIn("Closes #336", result.stdout)

    def test_renderer_discriminates_a_refs_only_issue_promoted_to_a_declaration(self):
        wrong = self.run_contract(
            "repository-closes", "--body-file", str(FIXTURES / "release-1108-refs-promoted.md")
        )
        self.assertEqual(wrong.returncode, 0, wrong.stderr)
        self.assertNotEqual(
            wrong.stdout.splitlines(),
            self.expected_closes(),
            "renderer echoed the body instead of discriminating",
        )
        self.assertIn("Closes #336", wrong.stdout)

    def test_reconcile_rejects_a_body_naming_an_issue_no_merged_commit_fixed(self):
        rejected = self.run_contract(
            "reconcile",
            "--body-file", str(FIXTURES / "release-1108-unfixed.md"),
            "--commits-file", str(FIXTURES / "release-1108-commits.json"),
            "--prs-file", str(FIXTURES / "release-1108-prs.json"),
        )
        self.assertNotEqual(rejected.returncode, 0)
        self.assertIn("#999", rejected.stderr)

    def test_reconcile_renders_the_closing_block_when_the_body_matches_the_range(self):
        accepted = self.run_contract(
            "reconcile",
            "--body-file", str(FIXTURES / "release-1108-pr.md"),
            "--commits-file", str(FIXTURES / "release-1108-commits.json"),
            "--prs-file", str(FIXTURES / "release-1108-prs.json"),
        )
        self.assertEqual(accepted.returncode, 0, accepted.stderr)
        self.assertEqual(accepted.stdout.splitlines(), self.expected_closes())

    def test_reconcile_rejects_a_body_that_drops_a_fixed_issue(self):
        dropped = self.run_contract(
            "reconcile",
            "--body-file", str(FIXTURES / "release-1108-integration-pr.md"),
            "--commits-file", str(FIXTURES / "release-1108-commits.json"),
            "--prs-file", str(FIXTURES / "release-1108-prs.json"),
        )
        self.assertNotEqual(dropped.returncode, 0)
        self.assertIn("#343", dropped.stderr)

    def test_integration_pr_body_may_not_carry_a_closing_keyword(self):
        """GitHub ignores Closes off the default branch, so on a release/1.10.8 PR
        it is not a harmless extra: it is a promise the merge cannot keep."""
        rejected = self.run_contract(
            "repository-closes",
            "--body-file", str(FIXTURES / "release-1108-integration-pr-closing.md"),
        )
        self.assertEqual(rejected.returncode, 2)
        self.assertIn("GitHub closing keywords are forbidden", rejected.stderr)

    def test_integration_pr_body_declares_its_fixed_issues(self):
        accepted = self.run_contract(
            "parse", "--body-file", str(FIXTURES / "release-1108-integration-pr.md")
        )
        self.assertEqual(accepted.returncode, 0, accepted.stderr)


class CommitClosureAuditTest(unittest.TestCase):
    """A closing keyword names an issue this range actually fixed, and every fixed
    issue is claimed by some commit. Both directions are checked, because an audit
    that only ever passes proves nothing about the range it ran over."""

    def run_contract(self, *args: str) -> subprocess.CompletedProcess:
        return subprocess.run(
            [sys.executable, str(SCRIPT), *args],
            cwd=ROOT,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )

    def audit(self, commits: str) -> subprocess.CompletedProcess:
        return self.run_contract(
            "audit-commits",
            "--commits-file", str(FIXTURES / commits),
            "--body-file", str(FIXTURES / "release-1108-pr.md"),
        )

    def test_clean_range_passes(self):
        result = self.audit("release-1108-commits.json")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_audit_fails_on_a_closing_keyword_outside_the_fixed_set(self):
        result = self.audit("release-1108-commits-false-close.json")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("#336", result.stderr)

    def test_audit_fails_on_a_public_issue_closed_by_full_url(self):
        """GitHub honours the issue URL as a closing keyword, so an audit that reads
        only the #N forms would let a merge close a public release issue."""
        result = self.audit("release-1108-commits-false-close.json")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("rollacode/codeterm-releases#64", result.stderr)
        self.assertIn("never on merge", result.stderr)

    def test_audit_fails_when_a_fixed_issue_has_no_commit_claiming_it(self):
        result = self.audit("release-1108-commits-unclaimed.json")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("#318", result.stderr)


class ClosureBoundaryAuthorityTest(unittest.TestCase):
    """`issue-closure-boundary.json` was accepted on presence, so v1.10.11 carried
    one minted on a day the validator never ran and range-start read it as proof
    of mirror verification. The asset is a claim; the validator run Actions still
    reports is the evidence."""

    VALIDATOR_PATH = ".github/workflows/release.yml"
    PREVIOUS_SHA = "1" * 40
    LATEST_SHA = "2" * 40

    def run_contract(self, *args: str) -> subprocess.CompletedProcess:
        return subprocess.run(
            [sys.executable, str(SCRIPT), *args],
            cwd=ROOT,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )

    def range_start(self, releases: list[dict]) -> subprocess.CompletedProcess:
        with tempfile.NamedTemporaryFile(
            "w", suffix=".json", delete=False, encoding="utf-8"
        ) as handle:
            json.dump(releases, handle)
            path = handle.name
        self.addCleanup(Path(path).unlink, missing_ok=True)
        return self.run_contract("range-start", "--releases-file", path)

    def release(self, version: str, sha: str, run_id: int | None, **overrides) -> dict:
        boundary = {
            "version": version,
            "source_sha": sha,
            "public_mirror_verified": True,
        }
        if run_id is not None:
            boundary["validator_run_id"] = run_id
        boundary.update(overrides.pop("boundary", {}))
        run = None
        if run_id is not None:
            run = {
                "id": run_id,
                "path": self.VALIDATOR_PATH,
                "conclusion": "success",
            }
            run.update(overrides.pop("run", {}))
        entry = {
            "version": version,
            "sha": sha,
            "private_published": True,
            "public_published": True,
            "closure_boundary": boundary,
            "validator_run": run,
        }
        entry.update(overrides)
        return entry

    def previous(self) -> dict:
        return self.release("1.10.10", self.PREVIOUS_SHA, 28021342987)

    def test_closure_boundary_with_a_verifiable_run_id_is_accepted(self):
        result = self.range_start(
            [self.previous(), self.release("1.10.11", self.LATEST_SHA, 33978726809)]
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), self.LATEST_SHA)

    def test_legacy_validator_run_remains_a_valid_boundary(self):
        latest = self.release(
            "1.10.11",
            self.LATEST_SHA,
            33978726809,
            run={"path": ".github/workflows/release-manifest-validate.yml"},
        )
        result = self.range_start([self.previous(), latest])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), self.LATEST_SHA)

    def test_closure_boundary_without_a_run_id_falls_back_to_the_previous_release(self):
        result = self.range_start(
            [self.previous(), self.release("1.10.11", self.LATEST_SHA, None)]
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), self.PREVIOUS_SHA)

    def test_closure_boundary_whose_run_actions_cannot_confirm_is_refused(self):
        latest = self.release("1.10.11", self.LATEST_SHA, 33978726809)
        latest["validator_run"] = None
        result = self.range_start([self.previous(), latest])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), self.PREVIOUS_SHA)

    def test_closure_boundary_naming_a_run_of_another_workflow_is_refused(self):
        latest = self.release(
            "1.10.11",
            self.LATEST_SHA,
            33978726809,
            run={"path": ".github/workflows/tests.yml"},
        )
        result = self.range_start([self.previous(), latest])
        self.assertEqual(result.stdout.strip(), self.PREVIOUS_SHA)

    def test_closure_boundary_naming_a_failed_validator_run_is_refused(self):
        latest = self.release(
            "1.10.11", self.LATEST_SHA, 33978726809, run={"conclusion": "failure"}
        )
        result = self.range_start([self.previous(), latest])
        self.assertEqual(result.stdout.strip(), self.PREVIOUS_SHA)

    def test_closure_boundary_minted_for_another_commit_is_refused(self):
        latest = self.release(
            "1.10.11", self.LATEST_SHA, 33978726809, boundary={"source_sha": "3" * 40}
        )
        result = self.range_start([self.previous(), latest])
        self.assertEqual(result.stdout.strip(), self.PREVIOUS_SHA)

    def test_closure_boundary_answering_for_a_different_run_is_refused(self):
        latest = self.release(
            "1.10.11", self.LATEST_SHA, 33978726809, run={"id": 12345}
        )
        result = self.range_start([self.previous(), latest])
        self.assertEqual(result.stdout.strip(), self.PREVIOUS_SHA)

    def test_no_release_carries_a_verifiable_closure_boundary(self):
        result = self.range_start([self.release("1.10.11", self.LATEST_SHA, None)])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), "")

    def test_a_refusal_names_why_the_boundary_is_not_evidence(self):
        sys.path.insert(0, str(ROOT / "scripts"))
        try:
            from release_issue_contract import closure_boundary_refusal
        finally:
            sys.path.pop(0)
        latest = self.release("1.10.11", self.LATEST_SHA, None)
        self.assertIsNotNone(closure_boundary_refusal(latest))
        self.assertIsNone(closure_boundary_refusal(self.previous()))


class SuiteDiscoveryTest(unittest.TestCase):
    """The entry point once sat mid-file, so classes below it never ran."""

    def test_every_test_case_in_this_module_is_discovered(self):
        module = sys.modules[__name__]
        defined = {
            name for name, value in vars(module).items()
            if isinstance(value, type) and issubclass(value, unittest.TestCase)
        }
        loaded = {
            type(case).__name__
            for suite in unittest.defaultTestLoader.loadTestsFromModule(module)
            for case in suite
        }
        self.assertEqual(defined, loaded, 'a TestCase in this module is not discovered')
        self.assertIn('Release1108ClosureContractTest', loaded)
        self.assertIn('CommitClosureAuditTest', loaded)


if __name__ == "__main__":
    unittest.main()
