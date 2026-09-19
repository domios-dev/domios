#!/usr/bin/env node
// Gate on closing a public rollacode/codeterm-releases issue.
//
// Merging is not shipping, and being the release owner is not evidence. The only
// thing that authorises the close is a passing published-manifest verification
// for THIS release, so this reads that record and refuses everything else.
//
// Usage:
//   check-release-closure.mjs --evidence <file> --version <x.y.z> [--issue <n>]...

import { readFile } from 'node:fs/promises';

// Published by every release with no escape hatch: darwin from the build
// machine, windows x64 from its matrix leg, linux-x86_64 from the AppImage lane.
const REQUIRED_PLATFORMS = [
  'darwin-aarch64',
  'darwin-x86_64',
  'windows-x86_64',
  'linux-x86_64',
];

// Not silently certified: each is absent for a documented reason, so a run
// reports which shipped instead of pretending the set is always complete.
const OPTIONAL_PLATFORMS = new Map([
  // .agents/skills/github-release/SKILL.md records shipping without it when the ARM64 runner is
  // unavailable and forbids synthesizing the asset; 1.7.2 shipped without it.
  ['windows-aarch64', 'the windows-build ARM64 runner was unavailable'],
  // publish-release.sh emits the fragment only when LINUX_ARM_SIG is set.
  ['linux-aarch64', 'the Linux ARM lane did not build it'],
]);

function parseArgs(argv) {
  const args = { evidence: null, version: null, issues: [] };
  for (let i = 2; i < argv.length; i++) {
    const flag = argv[i];
    if (flag === '--evidence') args.evidence = argv[++i];
    else if (flag === '--version') args.version = argv[++i];
    else if (flag === '--issue') args.issues.push(argv[++i]);
    else throw new Error(`Unknown argument: ${flag}`);
  }
  if (!args.evidence || !args.version) {
    throw new Error('Usage: --evidence <file> --version <x.y.z> [--issue <n>]...');
  }
  return args;
}

function refuse(reason) {
  console.error(`refusing to close: ${reason}`);
  process.exitCode = 1;
}

// A version segment in an arbitrary path is not a citation: the record must name
// the public release the shipped updater actually reads from.
const PUBLIC_MANIFEST = /^\/rollacode\/codeterm-releases\/releases\/download\/([^/]+)\/[^/]+$/;

function citedRelease(manifest) {
  if (typeof manifest !== 'string') return null;
  let parsed;
  try {
    parsed = new URL(manifest);
  } catch {
    return null;
  }
  if (parsed.protocol !== 'https:' || parsed.host !== 'github.com') return null;
  const asset = PUBLIC_MANIFEST.exec(parsed.pathname);
  if (asset === null) return null;
  return asset[1].startsWith('v') ? asset[1].slice(1) : asset[1];
}

async function readRecord(path) {
  try {
    const record = JSON.parse(await readFile(path, 'utf8'));
    return record !== null && typeof record === 'object' && !Array.isArray(record) ? record : null;
  } catch {
    return null;
  }
}

function missingPlatforms(record) {
  const verified = Array.isArray(record.platforms) ? record.platforms : [];
  return REQUIRED_PLATFORMS.filter((name) => !verified.includes(name));
}

async function main() {
  const args = parseArgs(process.argv);

  const record = await readRecord(args.evidence);
  if (!record) {
    refuse(
      `no published-manifest verification has run for v${args.version} `
      + `(${args.evidence} is missing, unreadable, or not a record)`,
    );
    return;
  }
  if (record.verified !== true) {
    const detail = (record.failures ?? []).join('; ') || 'no platforms verified';
    refuse(`the published-manifest verification failed: ${detail}`);
    return;
  }
  if (record.version !== args.version) {
    refuse(
      `the published-manifest verification belongs to v${record.version}, not v${args.version}`,
    );
    return;
  }
  // GitHub serves a replaced asset stale for minutes, so an uncached fetch can
  // verify the bytes the PREVIOUS run published and call this release good.
  if (record.cache_busted !== true) {
    refuse('the published-manifest verification ran without a cache-buster');
    return;
  }

  const cited = citedRelease(record.manifest);
  if (cited === null) {
    refuse(
      'the verification record does not cite a release-scoped manifest URL '
      + `(${record.manifest ?? '<absent>'})`,
    );
    return;
  }
  if (cited !== args.version) {
    refuse(`the verified manifest was served from the v${cited} release, not v${args.version}`);
    return;
  }

  const missing = missingPlatforms(record);
  if (missing.length > 0) {
    refuse(`the published manifest was not verified for ${missing.join(', ')}`);
    return;
  }

  const subjects = args.issues.length > 0
    ? args.issues.map((issue) => `rollacode/codeterm-releases#${issue}`).join(', ')
    : '(no issues named)';
  const absent = [...OPTIONAL_PLATFORMS]
    .filter(([name]) => !record.platforms.includes(name))
    .map(([name, reason]) => `${name} (${reason})`);
  console.log(
    `closure authorised for v${args.version}: ${subjects}\n`
    + `  verified ${record.platforms.join(', ')} at ${record.verified_at}\n`
    + `  optional platforms absent: ${absent.join('; ') || 'none'}\n`
    + `  against ${record.manifest}`,
  );
}

await main();
