#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const DEFAULT_SOURCE = "rollacode/codeterm";
const DEFAULT_MIRROR = "rollacode/codeterm-releases";

export function isPlaceholderBody(body, tag) {
  const text = body.trim();
  const version = tag.replace(/^v/, "");
  const exact = new Set([tag, `CodeTerm ${tag}`, `CodeTerm v${version}`]);
  return (
    text.length === 0 ||
    exact.has(text) ||
    /^Auto-mirrored release artifacts for CodeTerm\b/i.test(text) ||
    /^Auto-mirrored from rollacode\/codeterm\b/i.test(text) ||
    /^See the assets to download this version and install\./i.test(text)
  );
}

export function canonicalGeneratedBody(tag, generated) {
  const text = generated.trim();
  if (!text) throw new Error(`GitHub generated empty release notes for ${tag}`);
  if (/^## CodeTerm\b/.test(text)) return text;
  return `## CodeTerm ${tag.replace(/^v/, "")}\n\n${text}`;
}

export function isGeneratedBody(body) {
  return /(^|\n)## What's Changed\s*($|\n)/.test(body);
}

export function canonicalPullRequestBody(tag, pullRequestBody) {
  const cleaned = pullRequestBody
    .split(/\r?\n/)
    .filter((line) => !/^(Repository-Issue:|Public-Issue:|(?:Closes|Fixes|Refs)\s)/i.test(line.trim()))
    .join("\n")
    .trim();
  if (!cleaned) throw new Error(`release PR for ${tag} has an empty body`);
  const heading = `## CodeTerm ${tag.replace(/^v/, "")}`;
  return /^##\s+.+$/m.test(cleaned)
    ? cleaned.replace(/^##\s+.+$/m, heading)
    : `${heading}\n\n${cleaned}`;
}

function runGh(args, options = {}) {
  const result = spawnSync("gh", args, {
    encoding: "utf8",
    maxBuffer: 16 * 1024 * 1024,
    stdio: options.capture ? ["ignore", "pipe", "pipe"] : "inherit",
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    const detail = options.capture ? result.stderr.trim() : `exit ${result.status}`;
    throw new Error(`gh ${args.join(" ")} failed: ${detail}`);
  }
  return options.capture ? result.stdout : "";
}

function ghJson(args) {
  return JSON.parse(runGh(args, { capture: true }));
}

function ghText(args) {
  return runGh(args, { capture: true }).trim();
}

function release(repo, tag) {
  return ghJson(["api", `repos/${repo}/releases/tags/${tag}`]);
}

function generate(repo, tag) {
  return ghJson([
    "api",
    "--method",
    "POST",
    `repos/${repo}/releases/generate-notes`,
    "-f",
    `tag_name=${tag}`,
  ]).body;
}

function pullRequestNotes(repo, tag) {
  const sha = ghText(["api", `repos/${repo}/commits/${tag}`, "--jq", ".sha"]);
  const pulls = ghJson([
    "api",
    `repos/${repo}/commits/${sha}/pulls`,
    "--jq",
    "[.[] | {body}]",
  ]);
  const candidate = pulls.find((pull) => (pull.body ?? "").trim().length >= 80);
  return candidate ? canonicalPullRequestBody(tag, candidate.body) : "";
}

function writeBoth(sourceRepo, mirrorRepo, tag, body) {
  const dir = mkdtempSync(join(tmpdir(), "codeterm-release-notes-"));
  const file = join(dir, "notes.md");
  try {
    writeFileSync(file, `${body.trim()}\n`, "utf8");
    for (const repo of [sourceRepo, mirrorRepo]) {
      release(repo, tag);
      runGh(["release", "edit", tag, "--repo", repo, "--notes-file", file]);
    }
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

export function chooseCanonicalBody(tag, sourceBody, replacementBody) {
  const replace = isPlaceholderBody(sourceBody, tag) || isGeneratedBody(sourceBody);
  const body = replace ? replacementBody.trim() : sourceBody.trim();
  if (isPlaceholderBody(body, tag) || body.length < 80) {
    throw new Error(`release notes for ${tag} are still not meaningful`);
  }
  return body;
}

function prepareTag(sourceRepo, mirrorRepo, tag) {
  const source = release(sourceRepo, tag);
  release(mirrorRepo, tag);
  const existing = source.body ?? "";
  let replacement = "";
  if (isPlaceholderBody(existing, tag) || isGeneratedBody(existing)) {
    replacement = pullRequestNotes(sourceRepo, tag);
    if (!replacement) replacement = canonicalGeneratedBody(tag, generate(sourceRepo, tag));
  }
  const body = chooseCanonicalBody(tag, existing, replacement);
  return { tag, body };
}

function publishedTags(repo, count) {
  const releases = ghJson(["api", `repos/${repo}/releases?per_page=${Math.max(count, 30)}`]);
  return releases.filter((item) => !item.draft && item.published_at).slice(0, count).map((item) => item.tag_name);
}

function parseArgs(argv) {
  const options = { sourceRepo: DEFAULT_SOURCE, mirrorRepo: DEFAULT_MIRROR };
  for (let index = 0; index < argv.length; index += 1) {
    const value = argv[index];
    if (value === "--tag") options.tag = argv[++index];
    else if (value === "--backfill") options.backfill = Number(argv[++index]);
    else if (value === "--source-repo") options.sourceRepo = argv[++index];
    else if (value === "--mirror-repo") options.mirrorRepo = argv[++index];
    else throw new Error(`unknown argument: ${value}`);
  }
  if (Boolean(options.tag) === Boolean(options.backfill)) {
    throw new Error("pass exactly one of --tag TAG or --backfill COUNT");
  }
  if (options.backfill && (!Number.isInteger(options.backfill) || options.backfill < 1)) {
    throw new Error("--backfill must be a positive integer");
  }
  return options;
}

export function main(argv) {
  const options = parseArgs(argv);
  const tags = options.tag ? [options.tag] : publishedTags(options.sourceRepo, options.backfill);
  if (tags.length !== (options.backfill ?? 1)) {
    throw new Error(`expected ${options.backfill} published releases, found ${tags.length}`);
  }
  const updates = tags.map((tag) => prepareTag(options.sourceRepo, options.mirrorRepo, tag));
  for (const { tag, body } of updates) {
    writeBoth(options.sourceRepo, options.mirrorRepo, tag, body);
    process.stdout.write(`${tag}: ${body.length} characters synced to both releases\n`);
  }
}

if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
