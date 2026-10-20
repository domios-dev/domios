#!/usr/bin/env node
// Caller: CI tests.yml; scripts tests/check-archive-portable-test-paths.test.mjs; skill gates, github-release
// Fails when Rust test code reads a compile-time manifest or binary path at runtime outside its support helper.
import { readFileSync, readdirSync, statSync, existsSync } from 'node:fs';
import { dirname, join, relative, resolve, basename, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const SCAN_ROOTS = ['src-tauri', 'cli'];
const SKIP_DIRS = new Set(['target', 'node_modules', 'gen', 'pty-fork', 'vendor', '.git']);
const FORBIDDEN = /env!\(\s*"(CARGO_MANIFEST_DIR|CARGO_BIN_EXE_[A-Za-z0-9_-]+)"\s*\)/g;
const TEST_CFG = /#\[cfg\((?:test|(?:all|any)\([^\]]*\btest\b[^\]]*)\)\]/g;

function rustFiles(dir, out = []) {
  for (const name of readdirSync(dir)) {
    if (SKIP_DIRS.has(name)) continue;
    const path = join(dir, name);
    const stat = statSync(path);
    if (stat.isDirectory()) rustFiles(path, out);
    else if (name.endsWith('.rs')) out.push(path);
  }
  return out;
}

// Blanks comments and string/char literal bodies so structural scans ignore them.
export function mask(source) {
  const out = source.split('');
  const blank = (from, to) => { for (let k = from; k < to; k++) if (out[k] !== '\n') out[k] = ' '; };
  let i = 0;
  while (i < source.length) {
    const rest = source.slice(i, i + 3);
    if (rest.startsWith('//')) {
      const end = source.indexOf('\n', i);
      const stop = end < 0 ? source.length : end;
      blank(i, stop);
      i = stop;
    } else if (rest.startsWith('/*')) {
      let depth = 1;
      let j = i + 2;
      while (j < source.length && depth > 0) {
        if (source.startsWith('/*', j)) { depth++; j += 2; } else if (source.startsWith('*/', j)) { depth--; j += 2; } else j++;
      }
      blank(i, j);
      i = j;
    } else if (/^b?r#*"/.test(source.slice(i, i + 12)) && !/[A-Za-z0-9_]/.test(source[i - 1] ?? '')) {
      const head = /^b?r(#*)"/.exec(source.slice(i, i + 12));
      const close = `"${head[1]}`;
      const end = source.indexOf(close, i + head[0].length);
      const stop = end < 0 ? source.length : end + close.length;
      blank(i + head[0].length, stop - close.length);
      i = stop;
    } else if (source[i] === '"') {
      let j = i + 1;
      while (j < source.length && source[j] !== '"') j += source[j] === '\\' ? 2 : 1;
      blank(i + 1, j);
      i = j + 1;
    } else if (source[i] === "'") {
      const char = /^'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]+\}|.)|[^\\'])'/.exec(source.slice(i, i + 12));
      if (char) { blank(i + 1, i + char[0].length - 1); i += char[0].length; } else i++;
    } else i++;
  }
  return out.join('');
}

function closingIndex(masked, open) {
  const pairs = { '(': ')', '[': ']', '{': '}' };
  const stack = [];
  for (let i = open; i < masked.length; i++) {
    const c = masked[i];
    if (pairs[c]) stack.push(pairs[c]);
    else if (c === ')' || c === ']' || c === '}') {
      stack.pop();
      if (stack.length === 0) return i;
    }
  }
  return masked.length;
}

// The item an attribute applies to ends at its first top-level `;` or closing brace.
function itemEnd(masked, from) {
  let i = from;
  while (true) {
    while (/\s/.test(masked[i] ?? '')) i++;
    if (masked.startsWith('#[', i)) { i = closingIndex(masked, i + 1) + 1; continue; }
    break;
  }
  for (; i < masked.length; i++) {
    const c = masked[i];
    if (c === ';') return i;
    if (c === '(' || c === '[') i = closingIndex(masked, i);
    else if (c === '{') return closingIndex(masked, i);
  }
  return masked.length;
}

export function embeddedRanges(masked) {
  const ranges = [];
  for (const match of masked.matchAll(/\binclude_(?:str|bytes)!\s*\(/g)) {
    const open = match.index + match[0].length - 1;
    ranges.push([match.index, closingIndex(masked, open)]);
  }
  return ranges;
}

export function testRanges(masked) {
  const ranges = [];
  for (const match of masked.matchAll(TEST_CFG)) {
    ranges.push([match.index, itemEnd(masked, match.index + match[0].length)]);
  }
  return ranges;
}

// Files declared as `#[cfg(test)] mod name;` together with every descendant module file.
function testModuleFiles(file, masked, source) {
  const found = [];
  const ownDir = dirname(file);
  const stem = basename(file, '.rs');
  const childDir = ['mod', 'lib', 'main'].includes(stem) ? ownDir : join(ownDir, stem);
  for (const match of masked.matchAll(TEST_CFG)) {
    const end = itemEnd(masked, match.index + match[0].length);
    const item = masked.slice(match.index + match[0].length, end + 1);
    const decl = /\bmod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;$/.exec(item.trim());
    if (!decl) continue;
    const pathAttr = /#\[path\s*=\s*"([^"]*)"\]/.exec(source.slice(match.index, end + 1));
    if (pathAttr) {
      const target = resolve(ownDir, pathAttr[1]);
      found.push({ file: target, dir: join(dirname(target), basename(target, '.rs')) });
      continue;
    }
    const name = decl[1];
    for (const candidate of [join(childDir, `${name}.rs`), join(childDir, name, 'mod.rs')]) {
      if (existsSync(candidate)) found.push({ file: candidate, dir: join(childDir, name) });
    }
  }
  return found;
}

function isHelper(rel) {
  const parts = rel.split('/');
  return basename(rel) === 'test_support.rs' || parts.includes('support') || parts.includes('test_support');
}

function isTestPath(rel) {
  const parts = rel.split('/');
  const name = basename(rel, '.rs');
  return parts.includes('tests') || name === 'tests' || name.endsWith('_tests') || name.endsWith('_test')
    || parts.slice(0, -1).some((part) => part.endsWith('_test') || part.endsWith('_tests'));
}

const lineOf = (source, index) => source.slice(0, index).split('\n').length;
const inside = (ranges, index) => ranges.some(([from, to]) => index >= from && index <= to);

export function findViolations(root) {
  const files = SCAN_ROOTS.filter((dir) => existsSync(join(root, dir))).flatMap((dir) => rustFiles(join(root, dir)));
  const parsed = new Map();
  const testFiles = new Set();
  const testDirs = [];
  for (const file of files) {
    const source = readFileSync(file, 'utf8');
    const masked = mask(source);
    parsed.set(file, { source, masked });
    for (const entry of testModuleFiles(file, masked, source)) {
      testFiles.add(entry.file);
      testDirs.push(entry.dir + sep);
    }
  }
  const violations = [];
  for (const [file, { source, masked }] of parsed) {
    const rel = relative(root, file).split(sep).join('/');
    if (basename(file) === 'build.rs' || isHelper(rel)) continue;
    const wholeFile = isTestPath(rel) || testFiles.has(file) || testDirs.some((dir) => file.startsWith(dir));
    const tests = wholeFile ? null : testRanges(masked);
    const embedded = embeddedRanges(masked);
    for (const match of source.matchAll(FORBIDDEN)) {
      if (!masked.startsWith('env!', match.index) || inside(embedded, match.index)) continue;
      if (!wholeFile && !inside(tests, match.index)) continue;
      violations.push(`${rel}:${lineOf(source, match.index)}: ${match[0]}`);
    }
  }
  return violations.sort();
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const rootFlag = process.argv.indexOf('--root');
  const root = rootFlag >= 0 ? resolve(process.argv[rootFlag + 1]) : fileURLToPath(new URL('../', import.meta.url));
  const violations = findViolations(root);
  if (violations.length) {
    console.error('Rust test code reads a compile-time path that only exists on the nextest archive builder:');
    for (const line of violations) console.error(`  ${line}`);
    console.error('Use the crate test_support::fixture_manifest_dir() or tests/support/codeterm_bin.rs runtime helper.');
    process.exit(1);
  }
  console.log('Archive-portable test paths: OK');
}
