// Closing a public codeterm-releases issue claims the fix is in users' hands.
// Merging is not shipping, and being the release owner is not evidence.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const GUARD = path.join(ROOT, "scripts", "check-release-closure.mjs");

// Published with no escape hatch. windows-aarch64 and linux-aarch64 are absent
// on purpose: both have a documented reason a release may ship without them.
const REQUIRED_PLATFORMS = [
  "darwin-aarch64",
  "darwin-x86_64",
  "windows-x86_64",
  "linux-x86_64",
];

const MANIFEST_URL =
  "https://github.com/rollacode/codeterm-releases/releases/download/v1.10.8/latest.json?cb=1";

const PASSING = {
  verified: true,
  version: "1.10.8",
  cache_busted: true,
  manifest: MANIFEST_URL,
  platforms: REQUIRED_PLATFORMS,
  verified_at: "2026-09-01T10:00:00Z",
};

function scratch() {
  return mkdtempSync(path.join(tmpdir(), "release-closure-"));
}

function guard(args) {
  try {
    const stdout = execFileSync(process.execPath, [GUARD, ...args], {
      cwd: ROOT,
      encoding: "utf8",
    });
    return { code: 0, output: stdout };
  } catch (error) {
    return { code: error.status ?? 1, output: `${error.stdout ?? ""}${error.stderr ?? ""}` };
  }
}

function evidenceFile(dir, record, name = "evidence.json") {
  const file = path.join(dir, name);
  writeFileSync(file, JSON.stringify(record));
  return file;
}

function check(dir, record, name) {
  return guard(["--evidence", evidenceFile(dir, record, name), "--version", "1.10.8", "--issue", "55"]);
}

test("closure is authorised only by a passing verification of this release", () => {
  const dir = scratch();
  try {
    const result = check(dir, PASSING);
    assert.equal(result.code, 0, result.output);
    // The authorisation cites the run it rests on, so a close can be audited later.
    assert.match(result.output, /1\.10\.8/);
    assert.match(result.output, /cb=1/);
    assert.match(result.output, /55/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("closure is refused when no verification has run", () => {
  const dir = scratch();
  try {
    const result = guard([
      "--evidence", path.join(dir, "absent.json"),
      "--version", "1.10.8", "--issue", "55",
    ]);
    assert.equal(result.code, 1, result.output);
    assert.match(result.output, /no published-manifest verification has run/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("closure is refused when the verification failed", () => {
  const dir = scratch();
  try {
    const result = check(dir, {
      ...PASSING,
      verified: false,
      failures: ["darwin-aarch64: signature does not match the served bytes"],
    });
    assert.equal(result.code, 1, result.output);
    assert.match(result.output, /verification failed/);
    assert.match(result.output, /darwin-aarch64/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("closure is refused when the verification ran without a cache-buster", () => {
  const dir = scratch();
  try {
    // GitHub serves a replaced asset stale for minutes, so an uncached fetch can
    // verify the bytes the previous run published and call this release good.
    const result = check(dir, {
      ...PASSING,
      cache_busted: false,
      manifest:
        "https://github.com/rollacode/codeterm-releases/releases/download/v1.10.8/latest.json",
    });
    assert.equal(result.code, 1, result.output);
    assert.match(result.output, /without a cache-buster/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("closure is refused when the verification belongs to a previous release", () => {
  const dir = scratch();
  try {
    const result = check(dir, {
      ...PASSING,
      version: "1.10.7",
      manifest:
        "https://github.com/rollacode/codeterm-releases/releases/download/v1.10.7/latest.json?cb=1",
    });
    assert.equal(result.code, 1, result.output);
    assert.match(result.output, /belongs to v1\.10\.7/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("each of the four refusals names its own reason", () => {
  const dir = scratch();
  try {
    const records = [
      null,
      { ...PASSING, verified: false, failures: ["x"] },
      { ...PASSING, cache_busted: false },
      { ...PASSING, version: "1.10.7" },
    ];
    const reasons = new Set();
    records.forEach((record, index) => {
      const result = record === null
        ? guard(["--evidence", path.join(dir, "absent.json"), "--version", "1.10.8"])
        : check(dir, record, `case-${index}.json`);
      assert.equal(result.code, 1, result.output);
      reasons.add(result.output.trim());
    });
    assert.equal(reasons.size, 4, "refusals collapsed into fewer distinct reasons");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("closure is refused when the record cites no release-scoped manifest URL", () => {
  const dir = scratch();
  try {
    // A record that cannot say which release it verified is ambiguous, and
    // ambiguous evidence must not certify a release.
    const result = check(dir, { ...PASSING, manifest: "https://example.invalid/latest.json?cb=1" });
    assert.equal(result.code, 1, result.output);
    assert.match(result.output, /does not cite a release-scoped manifest URL/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("closure is refused when the record cites another release's manifest", () => {
  const dir = scratch();
  try {
    const result = check(dir, {
      ...PASSING,
      manifest:
        "https://github.com/rollacode/codeterm-releases/releases/download/v1.10.7/latest.json?cb=1",
    });
    assert.equal(result.code, 1, result.output);
    assert.match(result.output, /served from the v1\.10\.7 release/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("closure is refused when a required platform was never verified", () => {
  const dir = scratch();
  try {
    for (const absent of REQUIRED_PLATFORMS) {
      const result = check(
        dir,
        { ...PASSING, platforms: REQUIRED_PLATFORMS.filter((name) => name !== absent) },
        `without-${absent}.json`,
      );
      assert.equal(result.code, 1, result.output);
      assert.match(result.output, new RegExp(`was not verified for ${absent}`));
    }
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("a conditionally built platform does not block closure but is reported absent", () => {
  const dir = scratch();
  try {
    // Requiring either would refuse valid releases: 1.7.2 shipped without
    // windows-aarch64, and linux-aarch64 rides only on LINUX_ARM_SIG.
    const result = check(dir, PASSING);
    assert.equal(result.code, 0, result.output);
    assert.match(result.output, /optional platforms absent:.*windows-aarch64/);
    assert.match(result.output, /optional platforms absent:.*linux-aarch64/);
    assert.match(result.output, /ARM64 runner was unavailable/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("an optional platform that did ship is not reported absent", () => {
  const dir = scratch();
  try {
    const result = check(dir, {
      ...PASSING,
      platforms: [...REQUIRED_PLATFORMS, "windows-aarch64", "linux-aarch64"],
    });
    assert.equal(result.code, 0, result.output);
    assert.match(result.output, /optional platforms absent: none/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("a version segment in an arbitrary path is not a release citation", () => {
  const dir = scratch();
  try {
    const result = check(dir, {
      ...PASSING,
      manifest: "https://example.invalid/arbitrary/v1.10.8/latest.json?cb=1",
    });
    assert.equal(result.code, 1, result.output);
    assert.match(result.output, /does not cite a release-scoped manifest URL/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("a manifest cited from the private source repo is not public release evidence", () => {
  const dir = scratch();
  try {
    const result = check(dir, {
      ...PASSING,
      manifest:
        "https://github.com/rollacode/codeterm/releases/download/v1.10.8/latest.json?cb=1",
    });
    assert.equal(result.code, 1, result.output);
    assert.match(result.output, /does not cite a release-scoped manifest URL/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("an rc tag is a valid citation for its own version", () => {
  const dir = scratch();
  try {
    // build-updater-fragment.py strips the leading 'v', so the suffix rides in
    // the version and the tag segment still names the same release.
    const result = guard([
      "--evidence",
      evidenceFile(dir, {
        ...PASSING,
        version: "1.10.8-rc1",
        manifest:
          "https://github.com/rollacode/codeterm-releases/releases/download/v1.10.8-rc1/latest-rc.json?cb=1",
      }, "rc.json"),
      "--version", "1.10.8-rc1",
      "--issue", "55",
    ]);
    assert.equal(result.code, 0, result.output);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
