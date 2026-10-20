#!/usr/bin/env node
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const SCRIPT = fileURLToPath(import.meta.url);
const REPO = path.resolve(path.dirname(SCRIPT), '..');
const LEGACY = ['code', 'term'].join('');
const DEFAULT_ALLOWLIST = path.join(path.dirname(SCRIPT), 'rename-guard-allowlist.json');
const DEFAULT_OWNERSHIP = path.join(path.dirname(SCRIPT), 'rename-ownership.json');
const SCOPES = ['identity', 'cli', 'crates', 'native', 'docs'];

// WHAT: locate the former product identity in tracked paths and file content.
// WHY: six rename lanes need one strict, shared oracle with narrow exceptions.
// ORACLE: each occurrence is classified, allowed by an exact path/literal rule, or reported as a failure.

function normalize(file) { return file.replaceAll('\\', '/').replace(/^\.\//, ''); }

export function parseArgs(argv) {
  const args = { scope: 'all', report: false, root: REPO, allowlist: DEFAULT_ALLOWLIST, ownership: DEFAULT_OWNERSHIP, checkLane: null, baseRef: null };
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (flag === '--scope' && value && !args.scopeSet) { args.scope = value; args.scopeSet = true; index += 1; }
    else if (flag === '--root' && value) { args.root = path.resolve(value); index += 1; }
    else if (flag === '--allowlist' && value) { args.allowlist = path.resolve(value); index += 1; }
    else if (flag === '--ownership' && value) { args.ownership = path.resolve(value); index += 1; }
    else if (flag === '--check-lane' && value && !args.checkLane) { args.checkLane = value; index += 1; }
    else if (flag === '--base' && value) { args.baseRef = value; index += 1; }
    else if (flag === '--report') args.report = true;
    else throw new Error(`unexpected or incomplete argument: ${flag}`);
  }
  if (args.scope !== 'all' && !SCOPES.includes(args.scope)) throw new Error(`unknown scope: ${args.scope}`);
  if (args.checkLane && args.scopeSet) throw new Error('--scope cannot be combined with --check-lane');
  return args;
}

function globRegex(glob) {
  let source = '^';
  for (let index = 0; index < glob.length; index += 1) {
    const char = glob[index];
    if (char === '*' && glob[index + 1] === '*') {
      index += 1;
      if (glob[index + 1] === '/') { index += 1; source += '(?:.*/)?'; }
      else source += '.*';
    } else if (char === '*') source += '[^/]*';
    else if (char === '?') source += '[^/]';
    else source += char.replace(/[|\\{}()[\]^$+?.]/g, '\\$&');
  }
  return new RegExp(`${source}$`, 'i');
}

export function classifyScope(file) {
  const name = normalize(file).toLowerCase();
  const identity = [
    `src-tauri/crates/${LEGACY}-paths/`, `src-tauri/crates/${LEGACY}-config/`,
    `src-tauri/crates/${LEGACY}-secrets/`, `src-tauri/crates/${LEGACY}-mesh/`,
    'src-tauri/crates/domios-paths/', 'src-tauri/crates/domios-config/',
    'src-tauri/crates/domios-secrets/', 'src-tauri/crates/domios-mesh/',
  ];
  if (identity.some((prefix) => name.startsWith(prefix)) || name === 'cli/src/main.rs') return 'identity';
  if (name.startsWith('cli/') || name.startsWith('src-tauri/prompts/') || name.startsWith('prod-docs/')
    || name.startsWith('skills/') || name.startsWith('generated/cli/')
    || (/^plugins\/provider-[^/]+\/src\//.test(name) && /(?:hook|context|integration)/i.test(name))) return 'cli';
  if (name.startsWith('packages/') || name.endsWith('/cargo.toml') || name === 'cargo.toml'
    || name.endsWith('/package.json') || name === 'package.json'
    || /(?:^|\/)(?:plugin|manifest)\.(?:json|toml)$/.test(name)
    || name.startsWith('src-tauri/crates/')) return 'crates';
  if (/^src-tauri\/tauri[^/]*\.json$/.test(name) || name.startsWith('src-tauri/windows/')
    || name.startsWith('installer/') || /^scripts\/release-[^/]+\.mjs$/.test(name)
    || /^\.github\/workflows\/release[^/]*\.yml$/.test(name) || name === 'build.sh') return 'native';
  return 'docs';
}

function trackedFiles(root) {
  return execFileSync('git', ['ls-files', '-z'], { cwd: root, encoding: 'buffer', timeout: 30_000 })
    .toString('utf8').split('\0').filter(Boolean).map(normalize);
}

function occurrences(source, target = LEGACY) {
  const found = [];
  let offset = 0;
  while (offset <= source.length - target.length) {
    const at = source.indexOf(target, offset);
    if (at < 0) break;
    found.push(at);
    offset = at + Math.max(1, target.length);
  }
  return found;
}

function allowedOffsets(file, source, rules) {
  const allowed = new Set();
  for (const rule of rules) {
    if (!rule.matcher.test(file)) continue;
    const literal = rule.literal.toLowerCase();
    for (const literalOffset of occurrences(literal, LEGACY)) {
      let offset = 0;
      while (offset <= source.length - literal.length) {
        const at = source.indexOf(literal, offset);
        if (at < 0) break;
        allowed.add(at + literalOffset);
        offset = at + Math.max(1, literal.length);
      }
    }
  }
  return allowed;
}

function loadRules(file) {
  const parsed = JSON.parse(readFileSync(file, 'utf8'));
  if (!Array.isArray(parsed.rules)) throw new Error('allowlist.rules must be an array');
  return parsed.rules.map((rule, index) => {
    if (typeof rule.path !== 'string' || !rule.path || typeof rule.literal !== 'string' || !rule.literal
      || typeof rule.reason !== 'string' || !rule.reason.trim()) throw new Error(`invalid allowlist rule at index ${index}`);
    return { ...rule, matcher: globRegex(normalize(rule.path)) };
  });
}

export function scanTree({ root = REPO, allowlist = DEFAULT_ALLOWLIST, scope = 'all' } = {}) {
  const rules = loadRules(allowlist);
  const files = trackedFiles(root);
  const counts = Object.fromEntries([...SCOPES, 'all'].map((name) => [name, { files: 0, occurrences: 0, violations: 0 }]));
  const hits = [];
  for (const file of files) {
    const scopeName = classifyScope(file);
    const absolute = path.join(root, ...file.split('/'));
    const sources = [{ kind: 'path', text: file }];
    if (existsSync(absolute)) sources.push({ kind: 'content', text: readFileSync(absolute).toString('utf8') });
    const matches = sources.flatMap((source) => {
      const lowered = source.text.toLowerCase();
      const allowed = allowedOffsets(file, lowered, rules);
      return occurrences(lowered).map((index) => ({ file, scope: scopeName, source: source.kind, allowed: allowed.has(index) }));
    });
    if (!matches.length) continue;
    counts[scopeName].files += 1;
    counts.all.files += 1;
    for (const match of matches) {
      counts[scopeName].occurrences += 1;
      counts.all.occurrences += 1;
      if (!match.allowed) {
        counts[scopeName].violations += 1;
        counts.all.violations += 1;
      }
      hits.push(match);
    }
  }
  return { scope, counts, hits: hits.filter((hit) => scope === 'all' || hit.scope === scope), rules: rules.map(({ path: rulePath, literal, reason }) => ({ path: rulePath, literal, reason })) };
}

function runGit(root, args) {
  return execFileSync('git', args, { cwd: root, encoding: 'utf8', timeout: 30_000 }).trim();
}

function projectedBridgePath(file) {
  return normalize(file).split('/').map((segment) => {
    const lowered = segment.toLowerCase();
    if (lowered === LEGACY) return 'domios';
    if (lowered.startsWith(`${LEGACY}-`)) return `domios-${segment.slice(LEGACY.length + 1)}`;
    return segment;
  }).join('/');
}

function ownedPathMatches(candidate, allowed) {
  if (candidate === allowed || candidate.startsWith(allowed + '/')) return true;
  if (!allowed.includes('*') && !allowed.includes('?')) return false;
  return globRegex(allowed).test(candidate) || globRegex(allowed + '/**').test(candidate);
}

function ownedBy(file, owner) {
  const exact = normalize(file).toLowerCase();
  const projected = projectedBridgePath(file).toLowerCase();
  const matchesEntry = (entry) => {
    const declared = normalize(entry).toLowerCase().replace(/\/$/, '');
    const projectedDeclared = projectedBridgePath(declared).toLowerCase();
    const candidates = [exact, projected];
    const allowedPaths = [declared, projectedDeclared];
    return allowedPaths.some((allowed) => candidates.some((candidate) => ownedPathMatches(candidate, allowed)));
  };
  if ((owner.exclude_paths ?? []).some(matchesEntry)) return false;
  return owner.paths.some(matchesEntry);
}

export function checkLane({ root = REPO, ownership = DEFAULT_OWNERSHIP, taskKey, baseRef } = {}) {
  const manifest = JSON.parse(readFileSync(ownership, 'utf8'));
  const owners = Object.values(manifest.phases ?? {}).flat();
  const owner = owners.find((entry) => [entry.key, entry.id, entry.handle].includes(taskKey));
  if (!owner) throw new Error(`task key not found in ownership manifest: ${taskKey}`);
  const comparisonRef = baseRef ?? process.env.CODETERM_LANE_BASE ?? 'origin/rename/domios';
  const base = runGit(root, ['merge-base', 'HEAD', comparisonRef]);
  const rows = runGit(root, ['diff', '--name-status', '-z', `${base}...HEAD`]).split('\0');
  const changed = [];
  for (let index = 0; index < rows.length;) {
    const status = rows[index++];
    if (!status) continue;
    if (status.startsWith('R') || status.startsWith('C')) { changed.push(rows[index++], rows[index++]); }
    else changed.push(rows[index++]);
  }
  const changedPaths = [...new Set(changed.filter(Boolean).map(normalize))];
  const pathFailures = changedPaths.filter((file) => !ownedBy(file, owner));
  const occurrencesByOwner = trackedFiles(root).flatMap((file) => {
    if (!ownedBy(file, owner)) return [];
    const sources = [{ kind: 'path', text: file }];
    const absolute = path.join(root, ...file.split('/'));
    if (existsSync(absolute)) sources.push({ kind: 'content', text: readFileSync(absolute).toString('utf8') });
    const found = sources.flatMap((source) => occurrences(source.text.toLowerCase()).map(() => source.kind));
    return found.length ? [{ file, occurrences: found.length, sources: [...new Set(found)] }] : [];
  });
  const patch = runGit(root, ['diff', '--unified=0', `${base}...HEAD`, '--']);
  const bridgeLines = patch.split(/\r?\n/).filter((line) => /^[+-](?![+-]).*rename-bridge/i.test(line));
  const bridgeMode = owner.handle === 'backend-1-15-0-4' ? 'add' : owner.handle === 'infra-1-15-0-4' ? 'remove' : 'none';
  const bridgeFailures = bridgeLines.filter((line) => bridgeMode === 'none'
    || (bridgeMode === 'add' && !line.startsWith('+')) || (bridgeMode === 'remove' && !line.startsWith('-')));
  return { task: owner.key, phase: owner.phase, base, changed: changedPaths, pathFailures, occurrences: occurrencesByOwner, bridgeMode, bridgeLines, bridgeFailures,
    diagnostics: ownershipDiagnostics(root, manifest) };
}

function ownershipDiagnostics(root, manifest) {
  const tracked = trackedFiles(root).flatMap((file) => {
    const sources = [file];
    const absolute = path.join(root, ...file.split('/'));
    if (existsSync(absolute)) sources.push(readFileSync(absolute).toString('utf8'));
    return sources.some((source) => occurrences(source.toLowerCase()).length > 0) ? [file] : [];
  });
  const phases = {};
  for (const [phase, owners] of Object.entries(manifest.phases ?? {})) {
    const overlaps = [];
    const unowned = [];
    for (const file of tracked) {
      const matches = owners.filter((owner) => ownedBy(file, owner));
      if (matches.length === 0) unowned.push(file);
      if (matches.length > 1) overlaps.push({ file, tasks: matches.map((match) => match.key) });
    }
    phases[phase] = { overlaps, unowned };
  }
  return phases;
}

function printReport(result, reportOnly) {
  const scopes = result.scope === 'all' ? [...SCOPES, 'all'] : [result.scope];
  console.log(`rename-guard scope=${result.scope}`);
  for (const scope of scopes) {
    const count = result.counts[scope];
    console.log(`${scope}: files=${count.files} occurrences=${count.occurrences} violations=${count.violations}`);
  }
  if (!reportOnly) {
    for (const hit of result.hits.filter((item) => !item.allowed)) console.log(`FAIL ${hit.scope} ${hit.file} (${hit.source})`);
  }
}

function printLane(result) {
  console.log(`rename-guard lane=${result.task} phase=${result.phase} base=${result.base}`);
  for (const item of result.occurrences) console.log(`OCCURRENCE ${item.file}: ${item.occurrences} (${item.sources.join(', ')})`);
  for (const file of result.changed) console.log(`${result.pathFailures.includes(file) ? 'FAIL' : 'OK'} changed ${file}`);
  for (const [phase, diagnostic] of Object.entries(result.diagnostics)) {
    console.log(`phase ${phase}: overlaps=${diagnostic.overlaps.length} unowned=${diagnostic.unowned.length}`);
    for (const overlap of diagnostic.overlaps) console.log(`OVERLAP phase ${phase} ${overlap.file}: ${overlap.tasks.join(', ')}`);
    for (const file of diagnostic.unowned) console.log(`UNOWNED phase ${phase} ${file}`);
  }
  for (const line of result.bridgeFailures) console.log(`FAIL bridge ${line}`);
}

function main(argv) {
  const args = parseArgs(argv);
  if (args.checkLane) {
    const result = checkLane({ root: args.root, ownership: args.ownership, taskKey: args.checkLane, baseRef: args.baseRef });
    printLane(result);
    return result.pathFailures.length || result.bridgeFailures.length ? 1 : 0;
  }
  const result = scanTree({ root: args.root, allowlist: args.allowlist, scope: args.scope });
  printReport(result, args.report);
  return args.report ? 0 : result.counts[args.scope].violations ? 1 : 0;
}

if (process.argv[1] && path.resolve(process.argv[1]) === SCRIPT) {
  try { process.exitCode = main(process.argv.slice(2)); }
  catch (error) { console.error(`rename-guard: ${error.message}`); process.exitCode = 2; }
}
