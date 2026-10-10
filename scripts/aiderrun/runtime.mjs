import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync, mkdirSync, realpathSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const harness = path.dirname(fileURLToPath(import.meta.url));
import { commands, quote } from './model.mjs';

export function options(argv) {
  const result = {};
  for (let i = 0; i < argv.length; i += 2) {
    if (!['--lane', '--worker', '--orchestrator', '--surface'].includes(argv[i]) || !argv[i + 1]) throw new Error('expected --lane --worker --orchestrator --surface');
    result[argv[i].slice(2)] = argv[i + 1];
  }
  if (!path.isAbsolute(result.lane || '')) throw new Error('lane must be absolute');
  result.lane = realpathSync(result.lane);
  for (const key of ['worker', 'orchestrator']) if (!/^[a-zA-Z0-9_-]+$/.test(result[key] || '')) throw new Error(`missing exact ${key}`);
  if (result.orchestrator !== 'b3a2c8ff') throw new Error('wrong orchestrator');
  commands(result.surface);
  return result;
}

export function cli() {
  if (process.env.CODETERM_API_PORT !== '7686') throw new Error('requires dev Team environment');
  const binary = process.env.CODETERM_CLI_PATH || path.join(process.env.PATH.split(path.delimiter)[0], process.platform === 'win32' ? 'codeterm.exe' : 'codeterm');
  if (!binary || !path.isAbsolute(binary) || !/codeterm-dev/i.test(binary)) throw new Error('installed dev CLI path required');
  return binary;
}

export function call(binary, args, cwd, timeout = 7000) {
  const result = spawnSync(binary, args, { cwd, encoding: 'utf8', timeout, windowsHide: true, maxBuffer: 16 * 1024 * 1024 });
  if (result.error || result.status !== 0) throw new Error(`${binary}: ${result.error?.message || result.stderr || result.status}`);
  return result.stdout.trim();
}

export function fingerprint(lane) {
  const hash = createHash('sha256');
  hash.update(call('git', ['rev-parse', 'HEAD'], lane));
  hash.update(call('git', ['diff', '--binary', 'HEAD', '--', '.'], lane));
  const names = call('git', ['ls-files', '--others', '--exclude-standard', '-z'], lane).split('\0').filter(Boolean).sort();
  for (const name of names) { hash.update(name); hash.update(readFileSync(path.join(lane, name))); }
  return hash.digest('hex');
}

export function storage(binding) {
  const root = path.join(harness, '.runtime', binding.worker);
  mkdirSync(root, { recursive: true });
  return root;
}

export function readJson(file, fallback) {
  try { return JSON.parse(readFileSync(file, 'utf8').replace(/^\uFEFF/, '')); }
  catch (error) { if (error.code === 'ENOENT') return fallback; throw error; }
}

export function shellCommand(args, platform = process.platform) {
  return `${platform === 'win32' ? '& ' : ''}${args.map(value => quote(value, platform)).join(' ')}`;
}

export function save(file, value) { writeFileSync(file, JSON.stringify(value, null, 2), 'utf8'); }





