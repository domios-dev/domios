"""Structural contract for the release lanes' updater-signature gate.

Asserted against the parsed step list, not a grep: the flags must sit in one
step's own run script. The parser is dependency-free so the canonical gate
does not acquire an ambient PyYAML install.
"""

import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKFLOWS = ROOT / ".github" / "workflows"
VERIFIER = "scripts/verify-updater-manifest.mjs"
RELEASE_NOTES_SYNC = "scripts/sync-release-notes.mjs"
DARWIN_PLATFORMS = ("darwin-aarch64", "darwin-x86_64")

# Published with no escape hatch: darwin from the build machine, windows x64
# from its matrix leg, linux-x86_64 from the AppImage lane.
RELEASE_PLATFORMS = (
    "darwin-aarch64",
    "darwin-x86_64",
    "windows-x86_64",
    "linux-x86_64",
)

# windows-aarch64 may be dropped when the ARM64 runner is unavailable
# (docs/RELEASE.md; 1.7.2 shipped without it); linux-aarch64 rides LINUX_ARM_SIG.
OPTIONAL_PLATFORMS = ("windows-aarch64", "linux-aarch64")

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


class MacosSignatureGateTest(unittest.TestCase):
    def invocations(self) -> list[dict]:
        """Every check here goes through this. A test that iterates the step list
        without it passes on a lane that invokes the verifier nowhere at all."""
        found = verifier_steps(read("release-macos.yml"))
        self.assertTrue(
            found,
            "release-macos.yml never invokes the updater signature verifier",
        )
        return found

    def test_macos_lane_verifies_both_darwin_platforms(self):
        covered = set()
        for step in self.invocations():
            covered |= required_platforms(step)
        for platform in DARWIN_PLATFORMS:
            self.assertIn(platform, covered, f"macOS lane does not require {platform}")

    def test_no_macos_step_is_guarded_by_an_event_it_never_receives(self):
        """A gate whose condition no trigger can satisfy is not a gate: the mirror
        step carried a push predicate on a workflow_dispatch-only lane."""
        text = read("release-macos.yml")
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

    def test_macos_lane_pins_the_release_it_verifies(self):
        """Without --require-version this path never exercises the foreign-evidence
        refusal, so a previous release's manifest would satisfy it."""
        for step in self.invocations():
            self.assertIn("--require-version", step.get("run", ""))

    def test_linux_and_windows_lanes_still_verify(self):
        for name in ("release-linux.yml", "release-windows.yml"):
            with self.subTest(workflow=name):
                self.assertTrue(
                    verifier_steps(read(name)),
                    f"{name} lost its published-manifest verification",
                )


class ProductionPublicationGateTest(unittest.TestCase):
    """release.yml is the path that actually ships macOS. Its mirror check proved
    structure and HTTP reachability only, which is the state #58 was in."""

    def verifier_step(self) -> dict:
        invocations = verifier_steps(read("release.yml"))
        self.assertTrue(invocations, "release.yml never verifies the published manifest")
        return invocations[0]

    def test_published_mirror_verification_covers_every_shipped_platform(self):
        covered = required_platforms(self.verifier_step())
        for platform in RELEASE_PLATFORMS:
            self.assertIn(platform, covered, f"published-mirror check does not require {platform}")

    def test_conditionally_built_platforms_are_not_required(self):
        covered = required_platforms(self.verifier_step())
        for platform in OPTIONAL_PLATFORMS:
            self.assertNotIn(
                platform,
                covered,
                f"{platform} is built conditionally; requiring it refuses valid releases",
            )

    def test_published_manifest_is_fetched_with_a_cache_buster(self):
        self.assertIn("cb=", self.verifier_step().get("run", ""))

    def test_closure_evidence_is_minted_only_after_signature_verification(self):
        """issue-closure-boundary.json authorises closing public issues later."""
        run = self.verifier_step().get("run", "")
        self.assertIn("issue-closure-boundary.json", run, "boundary is minted in another step")
        self.assertLess(
            run.index(VERIFIER),
            run.index("issue-closure-boundary.json"),
            "closure evidence is written before the signatures are verified",
        )

    def test_closing_a_public_issue_is_guarded_by_the_verification_record(self):
        closing = [
            step for step in steps(read("release.yml"))
            if "gh issue close" in step.get("run", "")
        ]
        self.assertTrue(closing, "release.yml no longer closes public issues")
        for step in closing:
            run = step["run"]
            self.assertIn("check-release-closure.mjs", run, "close runs with no authority check")
            self.assertLess(run.index("check-release-closure.mjs"), run.index("gh issue close"))


class ReleaseNotesContractTest(unittest.TestCase):
    def test_full_notes_are_synced_after_publish_and_before_verification(self):
        parsed = steps(read("release.yml"))
        publishing = [index for index, step in enumerate(parsed) if "publish-release.sh" in step.get("run", "")]
        syncing = [index for index, step in enumerate(parsed) if RELEASE_NOTES_SYNC in step.get("run", "")]
        verifying = [index for index, step in enumerate(parsed) if VERIFIER in step.get("run", "")]
        self.assertEqual(len(publishing), 1, "release.yml must have one publication step")
        self.assertEqual(len(syncing), 1, "release.yml must sync one canonical notes body")
        self.assertEqual(len(verifying), 1, "release.yml must have one served-byte verifier")
        self.assertLess(publishing[0], syncing[0], "notes sync runs before releases exist")
        self.assertLess(syncing[0], verifying[0], "verification ran before both releases received notes")
        self.assertIn("--tag", parsed[syncing[0]].get("run", ""))


RELEASE_MANIFEST_VALIDATE = "release-manifest-validate.yml"

# The three independent, uncoordinated tag-triggered legs that publish the
# release. release-manifest-validate.yml must re-run after each of them
# completes so an incomplete matrix is caught no matter which leg finishes
# last (codeterm#383).
PUBLISHING_WORKFLOW_NAMES = (
    "Release (self-hosted build machine)",
    "release-windows",
    "release-linux",
)


class ManifestValidateTriggerTest(unittest.TestCase):
    """codeterm#383: a gate that only ever runs on workflow_dispatch is not a
    release-time gate — it is a checklist nobody is required to run. This
    workflow must fire on its own after every publishing leg completes, and
    must actually fail the run (not just print a warning) when the matrix it
    finds is incomplete."""

    def test_workflow_triggers_after_every_publishing_leg_completes(self):
        self.assertEqual(
            triggers(read(RELEASE_MANIFEST_VALIDATE)),
            {"workflow_run", "workflow_dispatch"},
            "release-manifest-validate.yml must run automatically after each publishing "
            "leg completes (workflow_run), in addition to workflow_dispatch for manual "
            "re-checks — a manual-only trigger is a checklist, not a gate",
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
