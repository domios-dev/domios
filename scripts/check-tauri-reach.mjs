import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const scope = 'src-tauri/crates/codeterm-service-core/src/';
export const baselinePath = 'scripts/tauri-reach-baseline.json';

export function countReach(source) {
  return [...source.matchAll(/tauri::|AppHandle|app_handle/g)].length;
}

export function scan(root) {
  const counts = {};
  function visit(directory) {
    for (const entry of readdirSync(join(root, directory), { withFileTypes: true })) {
      const file = `${directory}${entry.name}`;
      if (entry.isDirectory()) visit(`${file}/`);
      else if (entry.isFile() && file.endsWith('.rs')) {
        const count = countReach(readFileSync(join(root, file), 'utf8'));
        if (count) counts[file] = count;
      }
    }
  }
  visit(scope);
  const surface = "src-tauri/crates/codeterm-surface/src/";
  if (existsSync(join(root, surface))) visit(surface);
  return Object.fromEntries(Object.entries(counts).sort(([a], [b]) => a.localeCompare(b)));
}

export function validateBaseline(document) {
  if (!document || Object.keys(document).join(',') !== 'version,files' || document.version !== 1
      || !document.files || Array.isArray(document.files) || typeof document.files !== 'object') {
    throw new Error('invalid tauri reach baseline');
  }
  for (const [file, count] of Object.entries(document.files)) {
    if (!file.startsWith(scope) || !file.endsWith('.rs') || file.includes('\\')
        || file.split('/').some(part => !part || part === '..' || /[*?]/.test(part))
        || !Number.isSafeInteger(count) || count <= 0) {
      throw new Error(`invalid tauri reach baseline entry: ${file}`);
    }
  }
  return document.files;
}

export function checkReach(actual, allowed, previous = allowed) {
  const errors = [];
  for (const file of new Set([...Object.keys(actual), ...Object.keys(allowed)])) {
    const count = actual[file] ?? 0;
    const budget = allowed[file] ?? 0;
    if (count > budget) errors.push(`${file}: reach grew to ${count}, baseline ${budget}`);
    if (count < budget) errors.push(`${file}: stale baseline ${budget}, actual ${count}; lower or drop entry`);
    if (budget > (previous[file] ?? 0)) errors.push(`${file}: increased baseline ${previous[file] ?? 0} -> ${budget}`);
  }
  return errors;
}

function git(root, ...args) {
  return execFileSync('git', args, { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
}

export function main(root, args) {
  const allowed = validateBaseline(JSON.parse(readFileSync(join(root, baselinePath), 'utf8')));
  let base = process.env.TAURI_REACH_BASE || 'origin/develop';
  if (args.length) {
    if (args.length !== 2 || args[0] !== '--base') throw new Error('usage: check-tauri-reach.sh [--base REF]');
    base = args[1];
  }
  const fork = git(root, 'merge-base', 'HEAD', base);
  const hasBaseline = git(root, 'ls-tree', '--name-only', fork, '--', baselinePath);
  const previous = hasBaseline
    ? validateBaseline(JSON.parse(git(root, 'show', `${fork}:${baselinePath}`)))
    : allowed;
  const errors = checkReach(scan(root), allowed, previous);
  if (errors.length) throw new Error(errors.join('\n'));
  console.log(`tauri reach: ok (${Object.keys(allowed).length} files with remaining reach)`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    main(process.cwd(), process.argv.slice(2));
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
