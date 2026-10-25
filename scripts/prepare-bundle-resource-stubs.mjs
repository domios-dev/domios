#!/usr/bin/env node
import { existsSync, lstatSync, mkdirSync, readdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const markerName = '.codeterm-compile-stubs';
const content = 'CODETERM_COMPILE_STUB: never package this file\n';

function stageRoot(root) {
  return path.join(root, 'target', 'domios-package');
}

export function requiredPaths(target) {
  const windows = target.endsWith('-pc-windows-msvc');
  const mac = target.endsWith('-apple-darwin');
  const server = windows ? 'domios-server.exe' : 'domios-server';
  const cli = windows ? 'codeterm.exe' : 'codeterm';
  const files = [
    'server-component.json', `cli/${cli}`, `cli/${cli.replace('codeterm', 'domios')}`, 'dist/index.html',
    'LICENSE', 'NOTICE', `domios-server/${server}`,
  ];
  if (windows) {
    const arch = target.startsWith('aarch64-') ? 'arm64' : 'x64';
    files.push(`conpty/${arch}/OpenConsole.exe`, `conpty/${arch}/conpty.dll`);
  }
  if (mac) files.push(`domios-server/domios-server-${target}`);
  return files;
}

function checkedFile(file) {
  if (!existsSync(file)) return false;
  const stat = lstatSync(file);
  if (!stat.isFile() || stat.isSymbolicLink()) throw new Error(`compile resource is not a regular file: ${file}`);
  return true;
}

export function prepareCompileBundleStubs({ root = repo, target }) {
  if (!/^(x86_64|aarch64)-(apple-darwin|unknown-linux-gnu|pc-windows-msvc)$/.test(target ?? '')) {
    throw new Error(`unsupported compile-stub target: ${target}`);
  }
  const stage = stageRoot(root);
  if (existsSync(stage) && (!lstatSync(stage).isDirectory() || lstatSync(stage).isSymbolicLink())) {
    throw new Error(`compile resource stage is not a directory: ${stage}`);
  }
  const missing = requiredPaths(target).filter(relative => !checkedFile(path.join(stage, relative)));
  const skills = path.join(stage, 'skills');
  const missingSkills = !existsSync(skills) || readdirSync(skills).length === 0;
  if (missing.length === 0 && !missingSkills) return { created: 0, stage };
  mkdirSync(stage, { recursive: true });
  const marker = path.join(stage, markerName);
  if (!checkedFile(marker)) writeFileSync(marker, content, { flag: 'wx' });
  for (const relative of missing) {
    const file = path.join(stage, relative);
    mkdirSync(path.dirname(file), { recursive: true });
    if (!checkedFile(file)) writeFileSync(file, content, { flag: 'wx' });
  }
  if (missingSkills) {
    mkdirSync(skills, { recursive: true });
    const file = path.join(skills, markerName);
    if (!checkedFile(file)) writeFileSync(file, content, { flag: 'wx' });
  }
  return { created: missing.length + Number(missingSkills), stage };
}

export function assertNoCompileBundleStubs({ root = repo } = {}) {
  const marker = path.join(stageRoot(root), markerName);
  if (existsSync(marker)) throw new Error(`compile-only resource stage cannot be packaged: ${marker}`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const command = process.argv[2];
  if (command === 'prepare' && process.argv[3] === '--target' && process.argv[4]) {
    const result = prepareCompileBundleStubs({ target: process.argv[4] });
    process.stdout.write(`compile resource stubs created=${result.created}\n`);
  } else if (command === 'assert-package-ready' && process.argv.length === 3) {
    assertNoCompileBundleStubs();
  } else {
    throw new Error('usage: prepare-bundle-resource-stubs.mjs prepare --target <rust-target> | assert-package-ready');
  }
}
