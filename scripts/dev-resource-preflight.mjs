#!/usr/bin/env node
// Caller: build.sh gateway-fake|gateway-sleep|gateway-status|gateway-cancel|gateway-policy-probe; scripts check-dev-resource-preflight.sh, dev-prep.mjs; skill build
import fs from "node:fs";
import path from "node:path";
import { performance } from "node:perf_hooks";
import { cachedPreparation } from "./preparation-cache.mjs";

const requestedRoot = path.resolve(process.argv[2] ?? process.cwd());
const started = performance.now();

function fail(message) {
  console.error(`[dev-resource-preflight] ${message}`);
  process.exit(1);
}

function isInside(candidate, parent) {
  const relative = path.relative(parent, candidate);
  return relative !== "" && relative !== ".." && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative);
}

function lstat(candidate) {
  try {
    return fs.lstatSync(candidate);
  } catch (error) {
    if (error.code === "ENOENT") return null;
    throw error;
  }
}

function rejectLink(candidate, stat) {
  if (stat.isSymbolicLink()) fail(`frontendDist path contains a symlink, junction, or reparse point: ${candidate}`);
}

const rootStat = lstat(requestedRoot);
if (!rootStat?.isDirectory()) fail(`checkout root must be an existing directory: ${requestedRoot}`);
rejectLink(requestedRoot, rootStat);

const root = fs.realpathSync(requestedRoot);
const configPath = path.join(root, "src-tauri", "tauri.conf.json");
if (!fs.existsSync(configPath)) fail(`missing Tauri config: ${configPath}`);

const config = JSON.parse(fs.readFileSync(configPath, "utf8"));
const configured = config.build?.frontendDist;
if (typeof configured !== "string" || configured.length === 0 || /^https?:\/\//i.test(configured)) {
  fail("build.frontendDist must be a local directory path");
}

const frontendDist = path.resolve(path.dirname(configPath), configured);
if (!isInside(frontendDist, root)) fail(`frontendDist must stay inside this checkout: ${frontendDist}`);

const relativeParts = path.relative(root, frontendDist).split(path.sep);
let existingAncestor = root;
let missingPartIndex = relativeParts.length;
for (let index = 0; index < relativeParts.length; index += 1) {
  const candidate = path.join(existingAncestor, relativeParts[index]);
  const stat = lstat(candidate);
  if (!stat) {
    missingPartIndex = index;
    break;
  }
  rejectLink(candidate, stat);
  if (!stat.isDirectory()) fail(`frontendDist path component is not a directory: ${candidate}`);
  existingAncestor = candidate;
}

const realAncestor = fs.realpathSync(existingAncestor);
if (realAncestor !== root && !isInside(realAncestor, root)) {
  fail(`frontendDist ancestor resolves outside this checkout: ${realAncestor}`);
}
const missingParts = relativeParts.slice(missingPartIndex);
const validatedDestination = path.resolve(realAncestor, ...missingParts);
if (!isInside(validatedDestination, root)) {
  fail(`frontendDist missing path resolves outside this checkout: ${validatedDestination}`);
}

const reused = cachedPreparation(root, "frontend-dist", [configPath, frontendDist, new URL(import.meta.url),
  new URL("./preparation-cache.mjs", import.meta.url)], () => fs.mkdirSync(frontendDist, { recursive: true }));
const realFrontendDist = fs.realpathSync(frontendDist);
if (!isInside(realFrontendDist, root)) {
  fail(`frontendDist resolves outside this checkout: ${realFrontendDist}`);
}

console.log(`[dev-resource-preflight] worktree-local frontendDist ready: ${realFrontendDist}`);
console.log(`[test-loop] frontend-dist ${reused ? "unchanged" : "prepared"} ${(performance.now() - started).toFixed(1)}ms`);
