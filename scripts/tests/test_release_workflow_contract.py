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
    def test_macos_lane_verifies_both_darwin_platforms(self):
        invocations = verifier_steps(read("release-macos.yml"))
        self.assertTrue(
            invocations,
            "release-macos.yml never invokes the updater signature verifier",
        )
        covered = set()
        for step in invocations:
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
        for step in verifier_steps(read("release-macos.yml")):
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


class ManifestValidateTriggerTest(unittest.TestCase):
    """#47 is closeable only if this workflow runs at release time. It does not:
    it is manual-only, and no safe published-release trigger exists yet."""

    def test_workflow_is_manual_only(self):
        self.assertEqual(
            triggers(read("release-manifest-validate.yml")),
            {"workflow_dispatch"},
            "release-manifest-validate.yml gained a trigger; re-check whether it can run "
            "before every platform has mirrored before treating it as a release-time gate",
        )
