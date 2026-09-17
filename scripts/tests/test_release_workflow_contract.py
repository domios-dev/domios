"""Structural contract for the release lanes' updater-signature gate.

Asserted against the parsed step list, not a grep: the flags must sit in one
step's own run script. The parser is dependency-free so the canonical gate
does not acquire an ambient PyYAML install.
"""

import re
import subprocess
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from release_validator_armed import subscribed_workflows


ROOT = Path(__file__).resolve().parents[2]
WORKFLOWS = ROOT / ".github" / "workflows"
ORDER_CHECK = ROOT / "scripts" / "check-release-verification-order.sh"
VERIFIER = "scripts/verify-updater-manifest.mjs"
RELEASE_NOTES_SYNC = "scripts/sync-release-notes.mjs"

# Published with no escape hatch: darwin from the build machine, windows x64
# from its matrix leg, linux-x86_64 from the AppImage lane.
RELEASE_PLATFORMS = (
    "darwin-aarch64",
    "darwin-x86_64",
    "windows-x86_64",
    "linux-x86_64",
)

STEP_START = re.compile(r"^ {6}-\s+(.*)$")
STEP_KEY = re.compile(r"^ {8}([A-Za-z_][A-Za-z0-9_-]*):\s?(.*)$")
INLINE_KEY = re.compile(r"^([A-Za-z_][A-Za-z0-9_-]*):\s?(.*)$")
TRIGGER_KEY = re.compile(r"^ {2}([A-Za-z_][A-Za-z0-9_]*):")
ON_HEADER = re.compile(r'^"?on"?:\s*$')
BLOCK_SCALAR = {"|", ">", "|-", ">-", "|+", ">+"}
REQUIRE_PLATFORM = re.compile(r"--require-platform\s+([A-Za-z0-9_-]+)")
EVENT_TEST = re.compile(r"^github\.event_name\s*==\s*'([a-z_]+)'$")


def read(name: str) -> str:
    return (WORKFLOWS / name).read_text(encoding="utf-8")


def triggers(text: str) -> set[str]:
    lines = text.splitlines()
    for index, line in enumerate(lines):
        if not ON_HEADER.match(line):
            continue
        found = set()
        for candidate in lines[index + 1:]:
            if candidate.strip() == "":
                continue
            if not candidate.startswith("  "):
                break
            key = TRIGGER_KEY.match(candidate)
            if key:
                found.add(key.group(1))
        return found
    raise AssertionError("workflow declares no 'on:' block this parser recognises")


def _block_body(lines: list[str], start: int) -> tuple[str, int]:
    body: list[str] = []
    index = start
    while index < len(lines):
        line = lines[index]
        if line.strip() == "":
            body.append("")
            index += 1
            continue
        if len(line) - len(line.lstrip()) < 10:
            break
        body.append(line[10:])
        index += 1
    return "\n".join(body), index


def steps(text: str) -> list[dict]:
    lines = text.splitlines()
    parsed: list[dict] = []
    index = 0
    while index < len(lines):
        head = STEP_START.match(lines[index])
        if not head:
            index += 1
            continue
        step: dict[str, str] = {}
        inline = INLINE_KEY.match(head.group(1))
        if inline:
            step[inline.group(1)] = inline.group(2)
        index += 1
        while index < len(lines):
            line = lines[index]
            if line.strip() == "":
                index += 1
                continue
            if len(line) - len(line.lstrip()) <= 6:
                break
            key = STEP_KEY.match(line)
            if key and key.group(2).strip() in BLOCK_SCALAR:
                step[key.group(1)], index = _block_body(lines, index + 1)
                continue
            if key:
                step[key.group(1)] = key.group(2)
            index += 1
        parsed.append(step)
    if not parsed:
        raise AssertionError("no steps parsed; workflow indentation changed shape")
    return parsed


def verifier_steps(text: str) -> list[dict]:
    return [step for step in steps(text) if VERIFIER in step.get("run", "")]


def required_platforms(step: dict) -> set[str]:
    """The --require-platform values in this step's own run script."""
    return set(REQUIRE_PLATFORM.findall(step.get("run", "")))


def dead_condition(condition: str, events: set[str]) -> bool:
    """True when no disjunct of an `if:` can ever hold under these triggers."""
    expression = condition.strip()
    if expression.startswith("${{") and expression.endswith("}}"):
        expression = expression[3:-2].strip()
    if not expression:
        return False
    disjuncts = [part.strip() for part in expression.split("||")]
    for part in disjuncts:
        event = EVENT_TEST.match(part)
        if event is None or event.group(1) in events:
            return False
    return True


class PublisherCompletionSignalTest(unittest.TestCase):
    PUBLISHERS = (
        ("release.yml", "release-completion-release.json"),
        ("release-linux.yml", "release-completion-release-linux.json"),
        ("release-windows.yml", "release-completion-release-windows.json"),
        ("release-macos.yml", "release-completion-release-macos.json"),
    )

    def test_publishers_defer_served_byte_verification(self):
        for name, _ in self.PUBLISHERS:
            with self.subTest(workflow=name):
                self.assertFalse(
                    verifier_steps(read(name)),
                    f"{name} still verifies a manifest before all publishers finish",
                )

    def test_publishers_publish_named_completion_signals(self):
        for name, marker in self.PUBLISHERS:
            with self.subTest(workflow=name):
                text = read(name)
                self.assertIn("release-completion-v1", text)
                self.assertIn(marker, text)
        self.assertIn("release-completion-release-windows-x86_64.json", read("release-windows.yml"))

    def test_no_manual_only_step_is_guarded_by_an_event_it_never_receives(self):
        for name, _ in self.PUBLISHERS:
            text = read(name)
            events = triggers(text)
            for step in steps(text):
                condition = step.get("if", "")
                if not condition:
                    continue
                self.assertFalse(
                    dead_condition(condition, events),
                    f"step {step.get('name', '<unnamed>')!r} is guarded by {condition!r}, "
                    f"unsatisfiable under triggers {sorted(events)}",
                )


class ProductionPublicationGateTest(unittest.TestCase):
    """release.yml publishes a completion signal; the coordinating workflow owns
    the served-byte gate and closure authority."""

    def test_release_workflow_defers_served_byte_verification(self):
        self.assertFalse(verifier_steps(read("release.yml")))
        self.assertIn("needs: release", read("release.yml"))
        self.assertIn("release-completion-release.json", read("release.yml"))


    def test_closing_a_public_issue_is_guarded_by_the_verification_record(self):
        closing = [
            step for step in steps(read("release-manifest-validate.yml"))
            if "gh issue close" in step.get("run", "")
        ]
        self.assertTrue(closing, "manifest validation no longer closes public issues")
        for step in closing:
            run = step["run"]
            self.assertIn("check-release-closure.mjs", run, "close runs with no authority check")
            self.assertLess(run.index("check-release-closure.mjs"), run.index("gh issue close"))

    def test_closure_boundary_is_minted_after_served_byte_verification(self):
        parsed = steps(read(RELEASE_MANIFEST_VALIDATE))
        verifying = [index for index, step in enumerate(parsed) if VERIFIER in step.get("run", "")]
        minting = [index for index, step in enumerate(parsed) if "issue-closure-boundary.json" in step.get("run", "")]
        self.assertTrue(verifying)
        self.assertTrue(minting)
        self.assertLess(verifying[0], minting[0])


class ReleaseBuildSpeedContractTest(unittest.TestCase):
    RELEASE_LANES = ("release.yml", "release-windows.yml", "release-linux.yml")

    def test_release_lanes_do_not_cache_target_outputs(self):
        for name in self.RELEASE_LANES:
            with self.subTest(workflow=name):
                text = read(name)
                self.assertNotRegex(text, r"(?i)rust-cache|swatinem")

    def test_direct_tauri_lanes_pin_target_directory_target_and_features(self):
        for name in ("release-windows.yml", "release-linux.yml"):
            with self.subTest(workflow=name):
                text = read(name)
                self.assertIn("CARGO_TARGET_DIR: src-tauri/target", text)
                self.assertIn("CARGO_INCREMENTAL: '0'", text)
                self.assertIn("CODETERM_RELEASE_FEATURES: embeddings", text)
                self.assertIn(
                    "args: --target ${{ env.TARGET }} --features ${{ env.CODETERM_RELEASE_FEATURES }}",
                    text,
                )

    def test_release_input_prebuilds_the_app_and_embed_bins_with_the_same_contract(self):
        build = (ROOT / "build.sh").read_text(encoding="utf-8")
        self.assertIn('cargo_target_args=(--target "$CARGO_BUILD_TARGET")', build)
        self.assertIn('--features "${CODETERM_RELEASE_FEATURES:-embeddings}"', build)
        self.assertIn("--bin codeterm --bin codeterm-embed", build)

    def test_metadata_only_completion_jobs_run_on_the_self_hosted_mac_runner(self):
        for name in ("release.yml", "release-windows.yml"):
            with self.subTest(workflow=name):
                _, marker, tail = read(name).partition("  completion:\n")
                self.assertTrue(marker)
                header = tail.split("permissions:", 1)[0]
                self.assertIn("runs-on: [self-hosted, codeterm-builder]", header)
                self.assertNotIn("ubuntu-latest", header)
                self.assertNotIn("linux-build", header)

    def test_mac_pipeline_has_an_explicit_notarization_overlap_stage(self):
        script = (ROOT / ".codeterm" / "build-machine" / "scripts" / "mac-overlap.sh").read_text(
            encoding="utf-8"
        )
        self.assertIn('notarize-dmg.sh" "$ARM_DMG"', script)
        self.assertIn("ARM_NOTARY_PID=$!", script)
        self.assertIn('mac-bundle.sh" "$VER" x86_64', script)
        self.assertIn('wait "$ARM_NOTARY_PID"', script)


class ReleaseNotesContractTest(unittest.TestCase):
    def test_full_notes_are_synced_after_publish_and_before_verification(self):
        parsed = steps(read("release.yml"))
        publishing = [index for index, step in enumerate(parsed) if "publish-release.sh" in step.get("run", "")]
        syncing = [index for index, step in enumerate(parsed) if RELEASE_NOTES_SYNC in step.get("run", "")]
        verifying = [index for index, step in enumerate(parsed) if VERIFIER in step.get("run", "")]
        self.assertEqual(len(publishing), 1, "release.yml must have one publication step")
        self.assertEqual(len(syncing), 1, "release.yml must sync one canonical notes body")
        self.assertEqual(len(verifying), 0, "release.yml must defer served-byte verification")
        self.assertLess(publishing[0], syncing[0], "notes sync runs before releases exist")
        self.assertIn("release-completion-release.json", read("release.yml"))
        self.assertIn("--tag", parsed[syncing[0]].get("run", ""))


RELEASE_MANIFEST_VALIDATE = "release-manifest-validate.yml"

# The independent legs publish completion assets before this gate is triggered.
PUBLISHING_WORKFLOW_NAMES = (
    "Release (self-hosted build machine)",
    "release-windows",
    "release-linux",
    "release-macos",
)


class ManifestValidateTriggerTest(unittest.TestCase):
    """The coordinating gate waits for published completion assets and distinguishes
    pending matrix formation from a terminal publisher failure."""

    def test_workflow_triggers_after_every_publishing_leg_completes(self):
        self.assertEqual(
            triggers(read(RELEASE_MANIFEST_VALIDATE)),
            {"workflow_run", "workflow_dispatch"},
            "release-manifest-validate.yml must run automatically after each publishing "
            "leg completes (workflow_run), in addition to workflow_dispatch for manual "
            "re-checks",
        )

    def test_workflow_run_watches_every_publishing_leg(self):
        text = read(RELEASE_MANIFEST_VALIDATE)
        for name in PUBLISHING_WORKFLOW_NAMES:
            self.assertIn(
                f'"{name}"',
                text,
                f"release-manifest-validate.yml's workflow_run trigger does not name "
                f"{name!r}; an incomplete matrix from that leg would never be re-checked",
            )

    def test_latest_json_validation_covers_every_shipped_platform(self):
        found = verifier_steps(read(RELEASE_MANIFEST_VALIDATE))
        self.assertTrue(
            found,
            "release-manifest-validate.yml no longer invokes the updater signature "
            "verifier for latest.json — a format-only check is not proof of installability",
        )
        covered = set()
        for step in found:
            covered |= required_platforms(step)
        for platform in RELEASE_PLATFORMS:
            self.assertIn(
                platform,
                covered,
                f"release-manifest-validate.yml's latest.json check does not require "
                f"{platform}; an incomplete matrix on that platform would pass silently",
            )

    def test_debian_manifest_validation_requires_its_platform(self):
        found = verifier_steps(read(RELEASE_MANIFEST_VALIDATE))
        covered = set()
        for step in found:
            covered |= required_platforms(step)
        self.assertIn(
            "linux-x86_64",
            covered,
            "release-manifest-validate.yml must require linux-x86_64 somewhere "
            "(latest-deb.json's only platform) or a missing Debian artifact ships silently",
        )

    def test_incomplete_matrix_defers_without_invoking_verifier(self):
        text = read(RELEASE_MANIFEST_VALIDATE)
        self.assertIn("check-release-verification-order.sh", text)
        self.assertIn("MATRIX_COMPLETE:", text)
        self.assertIn("MATRIX_PENDING:", ORDER_CHECK.read_text(encoding="utf-8"))
        self.assertIn("steps.matrix.outputs.ready == 'true'", text)

    def test_terminal_matrix_failure_has_a_named_reason(self):
        self.assertIn("MATRIX_FAILED:", ORDER_CHECK.read_text(encoding="utf-8"))

    def test_ordering_uses_no_elapsed_time_as_completion(self):
        workflow = read(RELEASE_MANIFEST_VALIDATE)
        script = ORDER_CHECK.read_text(encoding="utf-8")
        self.assertNotIn("sleep", workflow.lower())
        self.assertNotIn("poll", workflow.lower())
        self.assertNotIn("sleep", script.lower())
        self.assertNotIn("poll", script.lower())

    def test_ordering_fixtures_cover_pending_complete_and_failure(self):
        fixtures = ROOT / "scripts" / "tests" / "fixtures" / "release-verification"
        for name in ("macos-only.json", "full.json", "never-completes.json"):
            with self.subTest(fixture=name):
                self.assertTrue((fixtures / name).is_file())
VALIDATOR_ARMED_CHECK = "scripts/release_validator_armed.py"
ARMED_FIXTURES = ROOT / "scripts" / "fixtures" / "release-validator-armed"


def completion_steps(text: str) -> list[dict]:
    """Steps of the leg's `completion` job — the last thing a publishing leg runs."""
    _, marker, tail = text.partition("  completion:\n")
    if not marker:
        raise AssertionError("publishing leg has no completion job")
    return steps(tail)


class ValidatorArmedGateTest(unittest.TestCase):
    """A workflow file on disk is not an armed workflow.

    Every contract test above reads the validator's source; none of them can see
    that GitHub had it disabled, which is how it went ten weeks without running
    while they all stayed green. Each publishing leg now asserts the validator's
    runtime state as its final step, so an unarmed validator fails the release
    instead of silently skipping it.
    """

    LEGS = ("release.yml", "release-linux.yml", "release-windows.yml", "release-macos.yml")

    def run_check(self, fixture: str) -> subprocess.CompletedProcess:
        return subprocess.run(
            [sys.executable, str(ROOT / VALIDATOR_ARMED_CHECK), str(ARMED_FIXTURES / fixture)],
            cwd=ROOT,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )

    def test_every_publishing_leg_ends_by_asserting_the_validator_is_armed(self):
        for name in self.LEGS:
            with self.subTest(workflow=name):
                parsed = completion_steps(read(name))
                self.assertIn(
                    VALIDATOR_ARMED_CHECK,
                    parsed[-1].get("run", ""),
                    f"{name}'s completion job does not end by asserting the validator is "
                    f"armed; a disabled validator would let this leg publish unvalidated",
                )

    def test_the_armed_assertion_survives_a_failed_publication(self):
        for name in self.LEGS:
            with self.subTest(workflow=name):
                step = completion_steps(read(name))[-1]
                self.assertIn(
                    "always()",
                    step.get("if", ""),
                    f"{name}'s armed assertion is skipped when the leg fails, which is "
                    f"exactly when an unvalidated release is most likely",
                )

    def test_the_armed_assertion_can_read_the_validators_runtime_state(self):
        for name in self.LEGS:
            with self.subTest(workflow=name):
                _, _, tail = read(name).partition("  completion:\n")
                self.assertIn(
                    "actions: read",
                    tail,
                    f"{name}'s completion job cannot query the validator's enabled state",
                )

    def test_the_armed_assertion_fails_the_leg_loudly(self):
        for name in self.LEGS:
            with self.subTest(workflow=name):
                run = completion_steps(read(name))[-1].get("run", "")
                self.assertIn("::error::", run, f"{name} reports an unarmed validator quietly")

    def test_an_active_subscribed_validator_is_armed(self):
        result = self.run_check("armed.json")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("VALIDATOR_ARMED", result.stdout)

    def test_a_disabled_validator_is_not_armed(self):
        for fixture in ("disabled.json", "disabled-by-inactivity.json"):
            with self.subTest(fixture=fixture):
                result = self.run_check(fixture)
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn("VALIDATOR_UNARMED", result.stdout)
                self.assertIn("gh workflow enable", result.stdout)

    def test_a_validator_absent_from_the_default_branch_is_not_armed(self):
        result = self.run_check("absent-from-default-branch.json")
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("VALIDATOR_UNARMED", result.stdout)

    def test_an_unreadable_enabled_state_is_not_armed(self):
        """A lookup that did not answer is not evidence that the validator will run."""
        result = self.run_check("unreadable-state.json")
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("VALIDATOR_UNARMED", result.stdout)
        self.assertNotIn("absent from", result.stdout)

    def test_a_manual_dispatch_only_validator_is_not_armed(self):
        result = self.run_check("manual-dispatch-only.json")
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("VALIDATOR_UNARMED", result.stdout)

    def test_a_leg_the_validator_does_not_watch_is_not_armed(self):
        result = self.run_check("unsubscribed-leg.json")
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("VALIDATOR_UNARMED", result.stdout)

    def leg_name(self, workflow: str) -> str:
        declared = re.search(r'^name:\s*(.+?)\s*$', read(workflow), re.MULTILINE)
        self.assertIsNotNone(declared, f"{workflow} declares no workflow name")
        return declared.group(1)

    def test_every_publishing_leg_name_is_one_the_validator_subscribes_to(self):
        """The leg reports `github.workflow`; a rename must not silently unsubscribe it.

        Read from the live validator, never a fixture snapshot: a stale copy would go
        green on exactly the drift this gate exists to catch.
        """
        watched = set(subscribed_workflows(read(RELEASE_MANIFEST_VALIDATE)))
        self.assertTrue(watched, "the validator subscribes to no publishing leg at all")
        for workflow in self.LEGS:
            with self.subTest(workflow=workflow):
                self.assertIn(
                    self.leg_name(workflow),
                    watched,
                    f"{workflow}'s workflow name is not in the validator's workflow_run list",
                )

    def test_each_live_leg_is_armed_against_the_live_validator(self):
        """End to end on today's files, with no snapshot in the way."""
        import json
        import tempfile

        for workflow in self.LEGS:
            with self.subTest(workflow=workflow):
                fixture = {
                    "validator": "release-manifest-validate",
                    "leg": self.leg_name(workflow),
                    "default_branch": "master",
                    "state": "active",
                    "default_branch_source": read(RELEASE_MANIFEST_VALIDATE),
                }
                with tempfile.TemporaryDirectory() as directory:
                    built = Path(directory) / "validator-armed.json"
                    built.write_text(json.dumps(fixture), encoding="utf-8")
                    result = subprocess.run(
                        [sys.executable, str(ROOT / VALIDATOR_ARMED_CHECK), str(built)],
                        cwd=ROOT,
                        text=True,
                        capture_output=True,
                        timeout=10,
                        check=False,
                    )
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertIn("VALIDATOR_ARMED", result.stdout)
