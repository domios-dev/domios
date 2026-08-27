/**
 * Dist freshness as a readiness proof.
 *
 * `12-web` specs test the SERVED bundle. Nothing in the harness proved that
 * bundle came from the tested HEAD: `build.sh` starts `vite build --watch` in
 * the background without awaiting it, `scripts/dev-static-server.mjs` waits for
 * `dist/index.html` to EXIST, and `dev_frontend_ready` only checks the served
 * HTML has a root div. A run could therefore serve the previous release's
 * bundle and pass readiness.
 *
 * The proof here is the dev provenance record `build.sh` already writes
 * (`write_dev_provenance` -> `<data dir>/dev-build-provenance.json`), which
 * refuses to be written unless its `buildSha` IS the current HEAD, plus two
 * artifact-ordering facts that catch the cases the record alone cannot.
 */
import fs from "node:fs";
import path from "node:path";
import { currentDevInstance, type DevInstance } from "./dev-instance";
import { REPO_ROOT, fetchHealth, testedHeadSha } from "./dev-console";

export const DEV_PROVENANCE_FILE = "dev-build-provenance.json";

const SKIPPED_DIRECTORIES = new Set([
  "node_modules",
  "dist",
  "target",
  "artifacts",
  ".git",
  ".turbo",
  "coverage",
]);

/** Everything Vite compiles into `dist/`. Discovered by walking, never pinned. */
const FRONTEND_SOURCE_ENTRIES = [
  "index.html",
  "vite.config.ts",
  "package.json",
  "pnpm-lock.yaml",
  "src",
  "public",
  "packages",
];

export interface DevProvenanceRecord {
  buildSha?: string;
  artifacts?: { frontend?: { path?: string; sha256?: string } };
}

export interface NewestFile {
  path: string;
  mtimeMs: number;
}

export interface DistFreshnessInput {
  headSha: string;
  healthBuildSha: string | null;
  record: DevProvenanceRecord | null;
  recordPath: string;
  recordMtimeMs: number | null;
  distDir: string;
  newestDistFile: NewestFile | null;
  newestSourceFile: NewestFile | null;
}

export type DistFreshnessReason =
  | "daemon_build_mismatch"
  | "dist_absent"
  | "provenance_absent"
  | "provenance_stale_head"
  | "provenance_other_dist"
  | "dist_rewritten_after_standup"
  | "dist_older_than_sources";

export type DistFreshnessVerdict =
  | { fresh: true }
  | { fresh: false; reason: DistFreshnessReason; detail: string };

/**
 * build.sh records `$(pwd -P)/dist` from git-bash, so the stored path can
 * arrive as `D:\repo\dist` or as MSYS `/d/repo/dist`. Both name one directory.
 */
export function normalizeComparablePath(value: string): string {
  const slashed = value.replace(/\\/g, "/").replace(/\/+$/, "");
  const msysDrive = /^\/([A-Za-z])\/(.*)$/.exec(slashed);
  const windows = msysDrive ? `${msysDrive[1]}:/${msysDrive[2]}` : slashed;
  return windows.toLowerCase();
}

function samePath(left: string, right: string): boolean {
  return normalizeComparablePath(left) === normalizeComparablePath(right);
}

/**
 * Pure. Every branch names the two values it compared so a failing run says
 * which artifact is stale, not merely that something is.
 */
export function evaluateDistFreshness(input: DistFreshnessInput): DistFreshnessVerdict {
  if (input.healthBuildSha !== input.headSha) {
    return {
      fresh: false,
      reason: "daemon_build_mismatch",
      detail: `daemon reports build_sha ${input.healthBuildSha ?? "absent"}, tested HEAD is ${input.headSha}`,
    };
  }
  if (!input.newestDistFile) {
    return { fresh: false, reason: "dist_absent", detail: `no built bundle under ${input.distDir}` };
  }
  if (!input.record || input.recordMtimeMs === null) {
    return {
      fresh: false,
      reason: "provenance_absent",
      detail: `no dev build provenance at ${input.recordPath}; start the instance with ./build.sh dev --wait`,
    };
  }
  if (input.record.buildSha !== input.headSha) {
    return {
      fresh: false,
      reason: "provenance_stale_head",
      detail: `provenance records build ${input.record.buildSha ?? "absent"}, tested HEAD is ${input.headSha}`,
    };
  }
  const recordedDist = input.record.artifacts?.frontend?.path;
  if (!recordedDist || !samePath(recordedDist, input.distDir)) {
    return {
      fresh: false,
      reason: "provenance_other_dist",
      detail: `provenance certifies ${recordedDist ?? "no frontend"}, the served bundle is ${input.distDir}`,
    };
  }
  if (input.newestDistFile.mtimeMs > input.recordMtimeMs) {
    return {
      fresh: false,
      reason: "dist_rewritten_after_standup",
      detail: `${input.newestDistFile.path} changed after readiness was certified (${input.newestDistFile.mtimeMs} > ${input.recordMtimeMs}); the served bundle is not the certified one`,
    };
  }
  if (input.newestSourceFile && input.newestSourceFile.mtimeMs > input.newestDistFile.mtimeMs) {
    return {
      fresh: false,
      reason: "dist_older_than_sources",
      detail: `${input.newestSourceFile.path} is newer than ${input.newestDistFile.path} (${input.newestSourceFile.mtimeMs} > ${input.newestDistFile.mtimeMs}); dist predates its sources`,
    };
  }
  return { fresh: true };
}

export function distFreshnessError(verdict: DistFreshnessVerdict): Error | null {
  if (verdict.fresh) return null;
  return new Error(
    `STALE WEB BUNDLE (${verdict.reason}): ${verdict.detail}. ` +
      "12-web specs test the served bundle; rebuild dist from the tested HEAD via ./build.sh dev --wait and rerun.",
  );
}

export function newestFileUnder(target: string, newest: NewestFile | null = null): NewestFile | null {
  let stat: fs.Stats;
  try {
    stat = fs.lstatSync(target);
  } catch {
    return newest;
  }
  if (stat.isFile()) {
    return !newest || stat.mtimeMs > newest.mtimeMs ? { path: target, mtimeMs: stat.mtimeMs } : newest;
  }
  if (!stat.isDirectory()) return newest;
  let current = newest;
  for (const entry of fs.readdirSync(target, { withFileTypes: true })) {
    if (entry.isDirectory() && SKIPPED_DIRECTORIES.has(entry.name)) continue;
    current = newestFileUnder(path.join(target, entry.name), current);
  }
  return current;
}

export function readDevProvenance(recordPath: string): { record: DevProvenanceRecord | null; mtimeMs: number | null } {
  try {
    const record = JSON.parse(fs.readFileSync(recordPath, "utf8")) as DevProvenanceRecord;
    return { record, mtimeMs: fs.statSync(recordPath).mtimeMs };
  } catch {
    return { record: null, mtimeMs: null };
  }
}

export function devProvenancePath(instance: DevInstance): string {
  const userProfile = process.env.USERPROFILE ?? process.env.HOME;
  if (!userProfile) throw new Error("cannot resolve the Playwright instance data directory");
  return path.join(userProfile, instance.dataDirName, DEV_PROVENANCE_FILE);
}

export async function collectDistFreshness(instance: DevInstance): Promise<DistFreshnessInput> {
  const distDir = path.join(REPO_ROOT, "dist");
  const recordPath = devProvenancePath(instance);
  const { record, mtimeMs } = readDevProvenance(recordPath);
  const health = await fetchHealth(instance);
  let newestSourceFile: NewestFile | null = null;
  for (const entry of FRONTEND_SOURCE_ENTRIES) {
    newestSourceFile = newestFileUnder(path.join(REPO_ROOT, entry), newestSourceFile);
  }
  return {
    headSha: testedHeadSha(),
    healthBuildSha: health?.build_sha ?? null,
    record,
    recordPath,
    recordMtimeMs: mtimeMs,
    distDir,
    newestDistFile: newestFileUnder(distDir),
    newestSourceFile,
  };
}

/** Fails loudly before any spec opens the served bundle. */
export async function assertServedBundleIsFresh(instance = currentDevInstance()): Promise<void> {
  const verdict = evaluateDistFreshness(await collectDistFreshness(instance));
  const error = distFreshnessError(verdict);
  if (error) throw error;
  console.log(`[dist-freshness] served bundle certified at HEAD ${testedHeadSha()}`);
}
