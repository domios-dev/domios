#!/usr/bin/env node
// Merge per-platform `latest.json` fragments emitted by each release workflow
// into a single channel manifest. Each fragment looks like:
//   { "version": "1.0.2", "notes": "...", "pub_date": "...",
//     "platforms": { "windows-x86_64": { "url": "...", "signature": "..." } } }
// The merged file unions the `platforms` maps and keeps the top-level metadata
// from the first fragment (all fragments in one release run share version etc).
//
// --base <latest.json> folds an existing manifest UNDER the fragments so a
// later-finishing workflow never drops a platform an earlier one published
// (read-modify-write race across the 3 independent release workflows). The base
// is only folded when its version matches the fragments' version — a stale
// previous-release manifest is discarded, never merged with mismatched URLs.

const fs = require('node:fs');
const path = require('node:path');

function parseArgs(argv) {
  const args = { in: null, out: null, base: null };
  for (let i = 2; i < argv.length; i++) {
    const flag = argv[i];
    if (flag === '--in') args.in = argv[++i];
    else if (flag === '--out') args.out = argv[++i];
    else if (flag === '--base') args.base = argv[++i];
    else throw new Error(`Unknown argument: ${flag}`);
  }
  if (!args.in || !args.out) {
    throw new Error('Usage: merge-latest-json.js --in <dir> --out <file> [--base <latest.json>]');
  }
  return args;
}

function findFragments(root) {
  const out = [];
  function walk(dir) {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) walk(full);
      else if (entry.isFile() && entry.name === 'latest.json') out.push(full);
    }
  }
  walk(root);
  return out;
}

function main() {
  const args = parseArgs(process.argv);
  const fragments = findFragments(args.in);
  if (fragments.length === 0) {
    throw new Error(`No latest.json fragments found under ${args.in}`);
  }

  const merged = { version: null, notes: null, pub_date: null, platforms: {} };
  for (const file of fragments) {
    const data = JSON.parse(fs.readFileSync(file, 'utf8'));
    if (!merged.version && data.version) merged.version = data.version;
    if (!merged.notes && data.notes) merged.notes = data.notes;
    if (!merged.pub_date && data.pub_date) merged.pub_date = data.pub_date;
  }

  if (!merged.version) {
    throw new Error('No version field found in any fragment');
  }

  // Fold the existing manifest first (monotonic), then overlay fresh fragments
  // so a fragment always wins for its own platform. Only when versions match —
  // a stale base from a prior release carries wrong-version URLs.
  if (args.base && fs.existsSync(args.base)) {
    try {
      const base = JSON.parse(fs.readFileSync(args.base, 'utf8'));
      if (base.version === merged.version && base.platforms) {
        Object.assign(merged.platforms, base.platforms);
        console.log(`Folded base manifest (v${base.version}, ${Object.keys(base.platforms).length} platform(s))`);
      } else if (base.version) {
        console.log(`Ignoring base manifest v${base.version} (!= fragments v${merged.version})`);
      }
    } catch (e) {
      console.log(`Base manifest unreadable, ignoring: ${e.message}`);
    }
  }

  for (const file of fragments) {
    const data = JSON.parse(fs.readFileSync(file, 'utf8'));
    if (data.platforms) Object.assign(merged.platforms, data.platforms);
  }

  if (Object.keys(merged.platforms).length === 0) {
    throw new Error('No platform entries found in any fragment');
  }

  fs.writeFileSync(args.out, JSON.stringify(merged, null, 2) + '\n');
  console.log(`Wrote ${args.out} with ${Object.keys(merged.platforms).length} platform(s):`);
  for (const key of Object.keys(merged.platforms)) console.log(`  - ${key}`);
}

main();
