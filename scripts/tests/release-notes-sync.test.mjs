import assert from "node:assert/strict";
import test from "node:test";

import {
  canonicalGeneratedBody,
  canonicalPullRequestBody,
  chooseCanonicalBody,
  isGeneratedBody,
  isPlaceholderBody,
} from "../sync-release-notes.mjs";

test("recognises release-body placeholders from every publishing lane", () => {
  for (const body of [
    "v1.10.8",
    "CodeTerm v1.10.8",
    "Auto-mirrored release artifacts for CodeTerm v1.10.8.",
    "Auto-mirrored from rollacode/codeterm. See the source repo for full release notes.",
    "See the assets to download this version and install. Auto-update users will be offered this build via the configured channel.",
  ]) {
    assert.equal(isPlaceholderBody(body, "v1.10.8"), true, body);
  }
});

test("preserves an authored source body byte-for-byte except trailing space", () => {
  const authored = "## CodeTerm 1.10.8\n\nA meaningful explanation of user-visible changes that is deliberately longer than eighty characters.";
  assert.equal(chooseCanonicalBody("v1.10.8", `${authored}\n`, "unused"), authored);
});

test("replaces a placeholder with generated notes and a version heading", () => {
  const generated = "## What's Changed\n\n* Fixed repository refresh after external changes.\n* Hardened release verification against served bytes.";
  const replacement = canonicalGeneratedBody("v1.10.8", generated);
  const body = chooseCanonicalBody("v1.10.8", "CodeTerm v1.10.8", replacement);
  assert.equal(body, replacement);
  assert.equal(isPlaceholderBody(body, "v1.10.8"), false);
});

test("uses the release PR body and removes issue-routing metadata", () => {
  const body = canonicalPullRequestBody(
    "v1.10.7",
    "## Release 1.10.7\n\n### Changes\n- Fixed updater selection.\n- Fixed orchestration ownership.\n\nRepository-Issue: rollacode/codeterm#1\nCloses #1",
  );
  assert.match(body, /^## CodeTerm 1\.10\.7/);
  assert.match(body, /Fixed updater selection/);
  assert.doesNotMatch(body, /Repository-Issue|Closes #1/);
});

test("recognises GitHub-generated notes as replaceable during backfill", () => {
  const body = "## CodeTerm 1.10.7\n\n## What's Changed\n* release: v1.10.7";
  assert.equal(isGeneratedBody(body), true);
  const replacement = "## CodeTerm 1.10.7\n\n### Changes\nA detailed release summary with enough information to explain the behavior that changed for users.";
  assert.equal(chooseCanonicalBody("v1.10.7", body, replacement), replacement);
});

test("refuses empty generated notes", () => {
  assert.throws(() => canonicalGeneratedBody("v1.10.8", ""), /generated empty/);
});

test("refuses a short body even when it is not a recognised placeholder", () => {
  assert.throws(
    () => chooseCanonicalBody("v1.10.8", "A tiny note", "unused"),
    /not meaningful/,
  );
});
