/**
 * WHAT: dist freshness is a proof, not an existence check.
 * WHY: build.sh never awaits `vite build --watch`, the static server waits for
 *      dist/index.html to EXIST, and dev_frontend_ready only sees a root div —
 *      so a 12-web spec could test the previous release's bundle silently.
 * ORACLE: each staleness shape yields its own reason; the clean shape is fresh.
 */
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import {
  DEV_PROVENANCE_FILE,
  devProvenancePath,
  distFreshnessError,
  evaluateDistFreshness,
  newestFileUnder,
  normalizeComparablePath,
  readDevProvenance,
  type DistFreshnessInput,
} from "./dist-freshness";
import { devInstanceForSlot } from "./dev-instance";

const HEAD = "a".repeat(40);
const OTHER_HEAD = "b".repeat(40);
const DIST = "D:/repo/dist";

function input(overrides: Partial<DistFreshnessInput> = {}): DistFreshnessInput {
  return {
    headSha: HEAD,
    healthBuildSha: HEAD,
    record: { buildSha: HEAD, artifacts: { frontend: { path: DIST, sha256: "c".repeat(64) } } },
    recordPath: `D:/data/${DEV_PROVENANCE_FILE}`,
    recordMtimeMs: 3_000,
    distDir: DIST,
    newestDistFile: { path: `${DIST}/assets/index-abc.js`, mtimeMs: 2_000 },
    newestSourceFile: { path: "D:/repo/src/main.tsx", mtimeMs: 1_000 },
    ...overrides,
  };
}

test("a bundle certified at the tested HEAD is fresh", () => {
  assert.deepEqual(evaluateDistFreshness(input()), { fresh: true });
  assert.equal(distFreshnessError({ fresh: true }), null);
});

test("a daemon built at another HEAD fails before the bundle is even considered", () => {
  const verdict = evaluateDistFreshness(input({ healthBuildSha: OTHER_HEAD }));
  assert.equal(verdict.fresh, false);
  assert.equal(verdict.fresh === false && verdict.reason, "daemon_build_mismatch");
  assert.equal(verdict.fresh === false && verdict.detail.includes(OTHER_HEAD), true);
});

test("a health payload without build_sha is a mismatch, not a pass", () => {
  const verdict = evaluateDistFreshness(input({ healthBuildSha: null }));
  assert.equal(verdict.fresh === false && verdict.reason, "daemon_build_mismatch");
});

test("no built bundle at all is a loud failure", () => {
  const verdict = evaluateDistFreshness(input({ newestDistFile: null }));
  assert.equal(verdict.fresh === false && verdict.reason, "dist_absent");
});

test("a bundle with no provenance record is unproven, so it fails", () => {
  const verdict = evaluateDistFreshness(input({ record: null, recordMtimeMs: null }));
  assert.equal(verdict.fresh === false && verdict.reason, "provenance_absent");
});

test("provenance from a previous HEAD is the stale-release case the rule exists for", () => {
  const verdict = evaluateDistFreshness(
    input({ record: { buildSha: OTHER_HEAD, artifacts: { frontend: { path: DIST } } } }),
  );
  assert.equal(verdict.fresh === false && verdict.reason, "provenance_stale_head");
  assert.equal(verdict.fresh === false && verdict.detail.includes(OTHER_HEAD), true);
});

test("a provenance path recorded from git-bash still names this dist", () => {
  const verdict = evaluateDistFreshness(
    input({
      distDir: "D:\\repo\\dist",
      record: { buildSha: HEAD, artifacts: { frontend: { path: "/d/repo/dist" } } },
    }),
  );
  assert.deepEqual(verdict, { fresh: true });
  assert.equal(normalizeComparablePath("/d/repo/dist"), "d:/repo/dist");
  assert.equal(normalizeComparablePath("D:\\repo\\dist\\"), "d:/repo/dist");
  assert.notEqual(normalizeComparablePath("/d/other/dist"), normalizeComparablePath("D:\\repo\\dist"));
});

test("provenance certifying a different dist directory does not certify this one", () => {
  const verdict = evaluateDistFreshness(
    input({ record: { buildSha: HEAD, artifacts: { frontend: { path: "D:/other/dist" } } } }),
  );
  assert.equal(verdict.fresh === false && verdict.reason, "provenance_other_dist");
});

test("a watch build that lands after readiness means the served bundle is uncertified", () => {
  const verdict = evaluateDistFreshness(
    input({ newestDistFile: { path: `${DIST}/index.html`, mtimeMs: 9_000 } }),
  );
  assert.equal(verdict.fresh === false && verdict.reason, "dist_rewritten_after_standup");
});

test("sources newer than the bundle prove the bundle predates them", () => {
  const verdict = evaluateDistFreshness(
    input({ newestSourceFile: { path: "D:/repo/src/App.tsx", mtimeMs: 2_500 } }),
  );
  assert.equal(verdict.fresh === false && verdict.reason, "dist_older_than_sources");
  assert.equal(verdict.fresh === false && verdict.detail.includes("App.tsx"), true);
});

test("the failure names the stale bundle and the remedy", () => {
  const error = distFreshnessError(evaluateDistFreshness(input({ healthBuildSha: OTHER_HEAD })));
  assert.notEqual(error, null);
  assert.match(String(error?.message), /STALE WEB BUNDLE \(daemon_build_mismatch\)/);
  assert.match(String(error?.message), /build\.sh dev --wait/);
});

test("an absent provenance file reads as absent rather than throwing", () => {
  const missing = readDevProvenance(`D:/definitely/absent/${DEV_PROVENANCE_FILE}`);
  assert.deepEqual(missing, { record: null, mtimeMs: null });
});

test("an unresolvable data-directory base raises instead of guessing", () => {
  const instance = devInstanceForSlot(0, 2);
  const profile = process.env.USERPROFILE;
  const home = process.env.HOME;
  delete process.env.USERPROFILE;
  delete process.env.HOME;
  try {
    assert.throws(() => devProvenancePath(instance), /cannot resolve the Playwright instance data directory/);
  } finally {
    if (profile !== undefined) process.env.USERPROFILE = profile;
    if (home !== undefined) process.env.HOME = home;
  }
});

test("the newest-file walk returns the newest file and skips build output", () => {
  assert.equal(newestFileUnder("D:/definitely/absent"), null);

  const root = mkdtempSync(path.join(tmpdir(), "codeterm-dist-freshness-"));
  mkdirSync(path.join(root, "nested"), { recursive: true });
  mkdirSync(path.join(root, "node_modules"), { recursive: true });
  const older = path.join(root, "nested", "older.ts");
  const newer = path.join(root, "nested", "newer.ts");
  const ignored = path.join(root, "node_modules", "ignored.ts");
  for (const [file, seconds] of [[older, 1_000], [newer, 2_000], [ignored, 9_000]] as const) {
    writeFileSync(file, "x");
    utimesSync(file, seconds, seconds);
  }

  const found = newestFileUnder(root);
  assert.equal(found?.path, newer);
  assert.equal(Math.round((found?.mtimeMs ?? 0) / 1_000), 2_000);
});
