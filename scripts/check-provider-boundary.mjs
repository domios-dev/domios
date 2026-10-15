#!/usr/bin/env node
// Caller: build.sh release-preflight/verify; CI file-sizes.yml; scripts qa-gates.lib.sh, tests/check-provider-boundary.test.mjs; skill gates
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const allowlistPath = 'scripts/provider-boundary-allowlist.json';
const migratedBaseline = '86b30145e90e9d2f31294ead517b82716fd44da8';
const scopes = ['src-tauri/src', 'src-tauri/crates', 'crates'];
const sourceExtension = /\.(rs|ts|tsx|js|mjs|cjs|py|swift|json|toml)$/;
const providerName = /codex|claude|opencode|aider|goose/i;

export function isCoreSource(file) {
  return scopes.some(scope => file.startsWith(`${scope}/`)) && sourceExtension.test(file);
}

export function detectOccurrences(file, source) {
  const lines = source.replaceAll('\r\n', '\n').split('\n');
  const occurrences = [];
  for (let line = 0; line < lines.length; line++) {
    for (const match of lines[line].matchAll(/[A-Za-z_$][A-Za-z0-9_$]*(?:-[A-Za-z0-9_$]+)*/g)) {
      const token = match[0];
      if (!providerName.test(token.replaceAll(/[_$-]/g, ''))) continue;
      occurrences.push({ file, token, line: line + 1 });
    }
  }
  return occurrences;
}

function key(entry) {
  return JSON.stringify([entry.file, entry.token]);
}

export function validateAllowlist(document) {
  if (!document || Object.keys(document).sort().join(',') !== 'entries,version' || document.version !== 1 || !Array.isArray(document.entries)) {
    throw new Error('allowlist must contain only version: 1 and entries');
  }
  const seen = new Set();
  for (const entry of document.entries) {
    if (!entry || Object.keys(entry).sort().join(',') !== 'count,file,justification,removalSlice,token') {
      throw new Error('entry requires exact file, token, count, justification and removalSlice');
    }
    if (typeof entry.file !== 'string' || !isCoreSource(entry.file) || /[\\*?\[\]{}]/.test(entry.file) || entry.file.split('/').some(part => !part || part === '.' || part === '..')) {
      throw new Error(`non-exact core source path: ${entry.file}`);
    }
    if (typeof entry.token !== 'string' || !/^[A-Za-z_$][A-Za-z0-9_$-]*$/.test(entry.token) || !providerName.test(entry.token.replaceAll(/[_$-]/g, ''))) {
      throw new Error(`non-exact provider token: ${entry.token}`);
    }
    if (!Number.isSafeInteger(entry.count) || entry.count < 1) {
      throw new Error('entry requires a positive occurrence count');
    }
    for (const field of ['justification', 'removalSlice']) {
      if (typeof entry[field] !== 'string' || !entry[field].trim()) throw new Error(`entry requires ${field}`);
    }
    const identity = key(entry);
    if (seen.has(identity)) throw new Error(`duplicate allowlist entry: ${entry.file} ${entry.token}`);
    seen.add(identity);
  }
  return document.entries;
}

export function countOccurrences(occurrences) {
  const counts = new Map();
  for (const occurrence of occurrences) {
    const identity = key(occurrence);
    const entry = counts.get(identity) ?? { file: occurrence.file, token: occurrence.token, count: 0 };
    entry.count++;
    counts.set(identity, entry);
  }
  return [...counts.values()].sort((a, b) => a.file.localeCompare(b.file) || a.token.localeCompare(b.token));
}

export function checkBoundary(occurrences, document, baseline) {
  const entries = validateAllowlist(document);
  const allowed = new Map(entries.map(entry => [key(entry), entry]));
  const original = new Map(baseline.map(entry => [key(entry), entry]));
  const actual = new Map(countOccurrences(occurrences).map(entry => [key(entry), entry]));
  const errors = [];
  for (const occurrence of actual.values()) {
    const allowance = allowed.get(key(occurrence))?.count ?? 0;
    if (occurrence.count > allowance) errors.push(`added occurrence: ${occurrence.file} ${occurrence.token}: ${occurrence.count} > ${allowance}`);
  }
  for (const entry of entries) {
    const count = actual.get(key(entry))?.count ?? 0;
    if (count === 0) errors.push(`unused entry: ${entry.file} ${entry.token} (remove it in the same commit)`);
    else if (count < entry.count) errors.push(`stale entry: ${entry.file} ${entry.token}: ${count} < ${entry.count} (lower it in the same commit)`);
    const previous = original.get(key(entry))?.count ?? 0;
    if (entry.count > previous) errors.push(`increased baseline: ${entry.file} ${entry.token}: ${entry.count} > ${previous}`);
  }
  return errors;
}

function git(root, args) {
  return execFileSync('git', ['-C', root, ...args], { encoding: 'utf8', maxBuffer: 128 * 1024 * 1024 });
}

export function scanWorkingTree(root) {
  const files = [...new Set(git(root, ['ls-files', '-z', '--cached', '--others', '--exclude-standard', '--', ...scopes]).split('\0'))].filter(isCoreSource).sort();
  return files.flatMap(file => {
    try {
      return detectOccurrences(file, readFileSync(resolve(root, file), 'utf8'));
    } catch (error) {
      if (error.code === 'ENOENT') return [];
      throw error;
    }
  });
}

export function scanRevision(root, revision) {
  let matches;
  try {
    matches = git(root, ['grep', '-l', '-z', '-I', '-i', '-E', 'codex|claude|open[_$-]*code|aider|goose', revision, '--', ...scopes]);
  } catch (error) {
    if (error.status === 1) return [];
    throw error;
  }
  const files = matches.split('\0').filter(Boolean).map(file => file.slice(revision.length + 1)).filter(isCoreSource);
  return files.flatMap(file => detectOccurrences(file, git(root, ['show', `${revision}:${file}`])));
}

// The baseline is where this branch left `base`; later moves on `base` must not read as increases here.
export function forkPointOf(root, base) {
  try {
    return git(root, ['merge-base', 'HEAD', base]).trim();
  } catch {
    throw new Error(`no fork point between HEAD and ${base}; fetch full history of both`);
  }
}

export function readBaseline(root, revision) {
  const resolved = git(root, ['rev-parse', '--verify', `${revision}^{commit}`]).trim();
  const files = git(root, ['ls-tree', '--name-only', resolved, '--', allowlistPath]).trim();
  if (!files) return countOccurrences(scanRevision(root, resolved));
  return validateAllowlist(JSON.parse(git(root, ['show', `${resolved}:${allowlistPath}`])));
}

export function baselineRevision(root, forkPoint, anchor = migratedBaseline) {
  try {
    git(root, ['merge-base', '--is-ancestor', anchor, 'HEAD']);
    git(root, ['merge-base', '--is-ancestor', forkPoint, anchor]);
    return anchor;
  } catch {
    return forkPoint;
  }
}

export function followRenames(root, revision, baseline, occurrences) {
  const fields = git(root, ['diff', '--name-status', '-z', '--break-rewrites', '--find-renames', '--find-copies-harder', revision, '--', ...scopes]).split('\0');
  const renamed = new Map();
  for (let index = 0; index < fields.length;) {
    const status = fields[index++];
    if (!status) continue;
    const from = fields[index++];
    if (status.startsWith('R') || status.startsWith('C')) {
      if (!renamed.has(from)) renamed.set(from, []);
      renamed.get(from).push(fields[index++]);
    }
  }
  const actual = new Map(countOccurrences(occurrences).map(entry => [key(entry), entry.count]));
  return baseline.flatMap(entry => {
    const destinations = renamed.get(entry.file);
    if (!destinations) return [entry];
    const retained = Math.min(entry.count, actual.get(key(entry)) ?? 0);
    let remaining = entry.count - retained;
    const allowances = retained ? [{ ...entry, count: retained }] : [];
    for (const file of destinations) {
      const count = Math.min(remaining, actual.get(key({ ...entry, file })) ?? 0);
      if (count) allowances.push({ ...entry, file, count });
      remaining -= count;
    }
    return allowances;
  });
}

export function seedAllowlist(occurrences) {
  return {
    version: 1,
    entries: countOccurrences(occurrences).map(entry => ({
      ...entry,
      justification: 'Existing develop provider coupling; temporary exact file/token count pending plugin extraction.',
      removalSlice: /codex/i.test(entry.token) ? 'Codex Chat-only runtime extraction (task_w9btoo)' : 'Remaining provider boundary extraction',
    })),
  };
}

function main() {
  const args = process.argv.slice(2);
  let base = process.env.PROVIDER_BOUNDARY_BASE ?? 'origin/develop';
  for (let i = 0; i < args.length; i++) {
    if (args[i] !== '--base' || !args[i + 1]) throw new Error('usage: node scripts/check-provider-boundary.mjs [--base REF]');
    base = args[++i];
  }
  const root = git(process.cwd(), ['rev-parse', '--show-toplevel']).trim();
  const document = JSON.parse(readFileSync(resolve(root, allowlistPath), 'utf8'));
  const occurrences = scanWorkingTree(root);
  const forkPoint = forkPointOf(root, base);
  const revision = baselineRevision(root, forkPoint);
  const errors = checkBoundary(occurrences, document, followRenames(root, revision, readBaseline(root, revision), occurrences));
  if (errors.length) throw new Error(errors.join('\n'));
  console.log(`Provider boundary PASS: ${occurrences.length} occurrences in ${document.entries.length} exact file/token pairs; baseline ${base} at ${revision}; fork point ${forkPoint}.`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(); } catch (error) {
    console.error(`Provider boundary FAIL: ${error.message}`);
    process.exitCode = 1;
  }
}
