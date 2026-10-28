import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { readFileSync, lstatSync, openSync, readSync, closeSync } from 'node:fs';
import { StringDecoder } from 'node:string_decoder';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const rules = [
  ['tailnet-ip', /\b100\.(?:6[4-9]|[7-9]\d|1[01]\d|12[0-7])\.(?:\d{1,3})\.(?:\d{1,3})\b/g],
  ['tailnet-dns', /\b[\w.-]+\.ts\.net\b/gi],
  ['tailnet-ipv6', /\bfd7a:115c:a1e0(?::[a-f\d]{0,4}){1,5}/gi],
  ['personal-path', /(?:[A-Z]:[\\/]+Users[\\/]+|(?<![\w/])\/(?:Users|home)\/)[\w.-]+(?:'[\w.-]+)*/gi],
  ['store-endpoint', /https?:\/\/[^\s"'<>`]+/gi],
  ['email', /\b[\w.+-]+@[\w.-]+\.[a-z]{2,}\b/gi],
  ['machine-id', /\b(?:[a-f\d]{2}:){5}[a-f\d]{2}\b/gi],
  ['machine-name', /\b(?:DESKTOP|LAPTOP)-[A-Z\d]{5,}\b/g],
  ['captured-report', /\bSTATUS:\s*(?:in_review|partial|blocked)\b/g],
];

export const fingerprint = value => createHash('sha256').update(value).digest('hex');

function candidate(rule, value) {
  if (rule === 'personal-path') {
    const principal = value.split(/[\\/]+/).at(-1).toLowerCase();
    return !/^(?:user\d*|test|alice|bob|runner|dev|ci|example|me|u|owner|a|b|foo|bar|shared|capture|fixture|probe|sample-user|source-user|peer-user|ann|anna|o'neil|o'neill|o'neil-.*)$/.test(principal);
  }
  if (rule === 'store-endpoint') {
    let url;
    try { url = new URL(value); } catch { return false; }
    const host = url.hostname;
    if (/^(?:localhost|127(?:\.\d+){3}|\[::1\])$/.test(host) || /(?:^|\.)(?:invalid|test|example|example\.(?:com|net|org))$/.test(host)) return false;
    return /(?:minio|s3[.-])/i.test(host) || /^(?:9000|9001)$/.test(url.port) || /^(?:10\.|192\.168\.|172\.(?:1[6-9]|2\d|3[01])\.)/.test(host);
  }
  if (rule === 'email') return !/(?:@|\.)(?:example\.(?:com|net|org)|invalid|test|localhost)$/i.test(value) && value !== 'git@github.com';
  return true;
}

export function scanText(path, text, allowances = []) {
  const findings = [];
  for (const [rule, pattern] of rules) {
    pattern.lastIndex = 0;
    for (const match of text.matchAll(pattern)) {
      if (rule === 'email' && text[match.index - 1] === '@' && /^[A-Z][A-Z0-9_]*@/.test(match[0])) continue;
      if (rule === 'captured-report' && !/\.(?:json|jsonl|ndjson|txt|cast)$/i.test(path)) continue;
      if (!candidate(rule, match[0])) continue;
      const digest = fingerprint(match[0]);
      if (allowances.some(entry => entry.path === path && entry.rule === rule && entry.sha256 === digest && entry.reason?.trim())) continue;
      findings.push({ path, line: text.slice(0, match.index).split('\n').length, rule, sha256: digest, offset: match.index });
    }
  }
  return findings;
}

function scanFile(absolute, path, expectedBytes, allowances) {
  const descriptor = openSync(absolute, 'r');
  const decoder = new StringDecoder('utf8');
  const buffer = Buffer.alloc(64 * 1024);
  const findings = [];
  let carry = '', lines = 0, bytes = 0;
  try {
    while (true) {
      const size = readSync(descriptor, buffer);
      bytes += size;
      if (bytes > expectedBytes) throw new Error('tracked file changed during scan');
      const text = carry + (size ? decoder.write(buffer.subarray(0, size)) : decoder.end());
      const cutoff = size ? Math.max(0, text.length - 4096) : text.length;
      findings.push(...scanText(path, text, allowances).filter(finding => finding.offset < cutoff).map(finding => ({ ...finding, line: finding.line + lines })));
      lines += text.slice(0, cutoff).split('\n').length - 1;
      carry = text.slice(cutoff);
      if (!size) break;
    }
    if (bytes !== expectedBytes) throw new Error('tracked file changed during scan');
  } finally { closeSync(descriptor); }
  return findings;
}

export function scanRepository(root, allowances) {
  const files = execFileSync('git', ['ls-files', '--cached', '-z'], { cwd: root, maxBuffer: 16 * 1024 * 1024 }).toString().split('\0').filter(Boolean);
  if (!files.length || files.length > 30000) throw new Error('tracked inventory outside scan bound');
  const findings = [];
  let bytes = 0;
  for (const path of files) {
    const absolute = resolve(root, path);
    const stat = lstatSync(absolute);
    if (!stat.isFile()) throw new Error('nonregular tracked entry');
    bytes += stat.size;
    if (bytes > 512 * 1024 * 1024) throw new Error('tracked tree exceeds scan bound');
    findings.push(...scanText(path, path, allowances), ...scanFile(absolute, path, stat.size, allowances));
  }
  return { files: files.length, bytes, findings };
}

function main() {
  if (process.argv.length !== 2) throw new Error('usage: node scripts/check-public-privacy.mjs (current tracked tree only)');
  const root = execFileSync('git', ['rev-parse', '--show-toplevel'], { encoding: 'utf8' }).trim();
  const allowances = JSON.parse(readFileSync(resolve(root, 'scripts/public-privacy-allowlist.json'), 'utf8'));
  if (!Array.isArray(allowances) || allowances.some(entry => typeof entry.path !== 'string' || !/^[a-f\d]{64}$/.test(entry.sha256) || !rules.some(([rule]) => rule === entry.rule) || !entry.reason?.trim())) throw new Error('invalid privacy policy');
  const result = scanRepository(root, allowances);
  for (const finding of result.findings) console.error(`${finding.path}:${finding.line}: ${finding.rule} [redacted]`);
  console.log(`public-privacy: ${result.files} tracked files, ${result.bytes} bytes, ${result.findings.length} findings`);
  process.exitCode = result.findings.length ? 1 : 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(); } catch { console.error('public-privacy: scan failed; coverage incomplete'); process.exitCode = 2; }
}
