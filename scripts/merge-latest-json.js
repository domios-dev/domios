#!/usr/bin/env node
// Merge per-platform `latest.json` fragments emitted by tauri-action into a
// single channel manifest. Each fragment looks like:
//   { "version": "1.0.2", "notes": "...", "pub_date": "...",
//     "platforms": { "windows-x86_64": { "url": "...", "signature": "..." } } }
// The merged file unions the `platforms` maps and keeps the top-level
// metadata from the first fragment encountered (all fragments share the
// same version/notes/pub_date in a single release run).

const fs = require('node:fs');
const path = require('node:path');

function parseArgs(argv) {
  const args = { in: null, out: null };
  for (let i = 2; i < argv.length; i++) {
    const flag = argv[i];
    if (flag === '--in') args.in = argv[++i];
    else if (flag === '--out') args.out = argv[++i];
    else throw new Error(`Unknown argument: ${flag}`);
  }
  if (!args.in || !args.out) {
    throw new Error('Usage: merge-latest-json.js --in <dir> --out <file>');
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
    if (data.platforms) Object.assign(merged.platforms, data.platforms);
  }

  if (!merged.version) {
    throw new Error('No version field found in any fragment');
  }
  if (Object.keys(merged.platforms).length === 0) {
    throw new Error('No platform entries found in any fragment');
  }

  fs.writeFileSync(args.out, JSON.stringify(merged, null, 2) + '\n');
  console.log(`Wrote ${args.out} with ${Object.keys(merged.platforms).length} platform(s):`);
  for (const key of Object.keys(merged.platforms)) console.log(`  - ${key}`);
}

main();
