#!/usr/bin/env node
import { execFileSync } from 'node:child_process';
import { readFileSync, readdirSync, mkdtempSync, rmSync, appendFileSync } from 'node:fs';
import { tmpdir, platform, arch } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export function machine(bytes) {
  if (bytes.length < 64) throw new Error('Truncated executable');
  if (bytes[0] === 0x4d && bytes[1] === 0x5a) {
    const offset = bytes.readUInt32LE(0x3c);
    if (offset + 6 > bytes.length || bytes.toString('binary', offset, offset + 4) !== 'PE\0\0') throw new Error('Invalid PE header');
    return { format: 'PE', arch: ({ 0x8664: 'x64', 0xaa64: 'arm64' })[bytes.readUInt16LE(offset + 4)] };
  }
  if (bytes.subarray(0, 4).equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46]))) {
    if (bytes[4] !== 2 || bytes[5] !== 1) throw new Error('Expected ELF64 little endian');
    return { format: 'ELF', arch: ({ 62: 'x64', 183: 'arm64' })[bytes.readUInt16LE(18)] };
  }
  if (bytes.readUInt32LE(0) === 0xfeedfacf) {
    return { format: 'Mach-O', arch: ({ 0x01000007: 'x64', 0x0100000c: 'arm64' })[bytes.readUInt32LE(4)] };
  }
  throw new Error('Unsupported executable format');
}
export function assertMachine(bytes, expected, format) {
  const actual = machine(bytes);
  if (actual.arch !== expected || actual.format !== format) throw new Error(`Expected ${format}/${expected}, got ${actual.format}/${actual.arch}`);
}
function files(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap(entry => {
    const full = path.join(dir, entry.name);
    return entry.isDirectory() ? files(full) : entry.isFile() ? [full] : [];
  });
}
function one(items, predicate, label) {
  const matches = items.filter(predicate);
  if (matches.length !== 1) throw new Error(`Expected one ${label}, found ${matches.length}`);
  return matches[0];
}
function run(command, args, cwd) {
  return execFileSync(command, args, { cwd, encoding: 'utf8', timeout: 120000, maxBuffer: 8 * 1024 * 1024 });
}
export function checkContents(appBytes, sha) {
  if (!/^[a-f0-9]{40}$/.test(sha) || !appBytes.includes(Buffer.from(sha))) throw new Error('App does not contain exact source SHA');
  const plugins = readdirSync(path.join(root, 'plugins')).filter(name => name.startsWith('provider-') || ['analytics', 'browser'].includes(name));
  for (const name of plugins) {
    for (const filename of ['plugin.json', 'plugin.js']) {
      const content = readFileSync(path.join(root, 'plugins', name, filename));
      if (!content.length || !appBytes.includes(content)) throw new Error(`Missing embedded plugin bytes: ${name}/${filename}`);
    }
  }
}
export function smoke(kind, directory, sha, expectedArch) {
  const version = JSON.parse(readFileSync(path.join(root, 'package.json'))).version;
  const scratch = mkdtempSync(path.join(tmpdir(), 'release-bundle-check-'));
  try {
    const artifacts = files(path.resolve(directory));
    let format;
    if (kind === 'macos') {
      const bundle = one(artifacts, file => file.endsWith('.app.tar.gz'), 'macOS updater archive');
      expectedArch = bundle.includes('aarch64') ? 'arm64' : 'x64';
      run('tar', ['-xzf', bundle, '-C', scratch]);
      format = 'Mach-O';
      const app = path.join(scratch, 'CodeTerm.app');
      run('codesign', ['--verify', '--deep', '--strict', app]);
      const dmg = one(artifacts, file => file.endsWith('.dmg'), 'notarized DMG');
      run('xcrun', ['stapler', 'validate', dmg]);
      const plistVersion = run('/usr/libexec/PlistBuddy', ['-c', 'Print :CFBundleShortVersionString', path.join(app, 'Contents/Info.plist')]).trim();
      if (plistVersion !== version) throw new Error(`Bundle version ${plistVersion} != ${version}`);
    } else if (kind === 'linux') {
      const image = one(artifacts, file => file.endsWith('.AppImage'), 'AppImage');
      run(image, ['--appimage-extract'], scratch);
      format = 'ELF';
      expectedArch ||= arch();
    } else if (kind === 'windows') {
      const installer = one(artifacts, file => file.endsWith('-setup.exe'), 'NSIS installer');
      run(process.env.RELEASE_7ZIP || '7z', ['x', '-y', `-o${scratch}`, installer]);
      format = 'PE';
      if (!['x64', 'arm64'].includes(expectedArch)) throw new Error('Windows target architecture required');
    } else throw new Error(`Unknown platform ${kind}`);
    const extracted = files(scratch);
    const app = one(extracted, file => /[/\\](?:codeterm|CodeTerm)(?:\.exe)?$/.test(file) && !/[/\\]cli[/\\]/.test(file), 'app executable');
    const cli = one(extracted, file => /[/\\]cli[/\\]codeterm(?:-[^.]+)?(?:\.exe)?$/.test(file), 'embedded CLI');
    const embed = one(extracted, file => /[/\\]codeterm-embed(?:-[^.]+)?(?:\.exe)?$/.test(file), 'embed executable');
    for (const executable of [app, cli, embed]) assertMachine(readFileSync(executable), expectedArch, format);
    checkContents(readFileSync(app), sha);
    one(extracted, file => file.endsWith(`${path.sep}dist${path.sep}index.html`), 'frontend entry point');
    if (kind === 'linux') {
      const deb = one(artifacts, file => file.endsWith('.deb'), 'Debian package');
      const debVersion = run('dpkg-deb', ['--field', deb, 'Version']).trim();
      if (debVersion !== version) throw new Error(`Debian version ${debVersion} != ${version}`);
      const debRoot = path.join(scratch, 'deb');
      run('dpkg-deb', ['--extract', deb, debRoot]);
      const debFiles = files(debRoot);
      for (const executable of [app, cli, embed]) {
        const basename = path.basename(executable);
        const packaged = one(debFiles, file => path.basename(file) === basename, `Debian ${basename}`);
        const content = readFileSync(packaged);
        assertMachine(content, expectedArch, format);
        if (executable === app) checkContents(content, sha);
      }
      one(debFiles, file => file.endsWith(`${path.sep}dist${path.sep}index.html`), 'Debian frontend entry point');
    }
    if (kind === 'windows') {
      const statuses = run('pwsh', ['-NoProfile', '-File', path.join(root, 'scripts/release-windows-artifact.ps1'), '-App', app, '-Cli', cli, '-Embed', embed, '-Version', version]);
      console.log(`Authenticode (not required): ${statuses}`);
      if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, `Authenticode (not required): ${statuses}\n`);
    }
    const canExecute = expectedArch === arch() || (platform() === 'darwin' && arch() === 'arm64' && expectedArch === 'x64');
    if (canExecute) {
      const output = run(cli, ['--version']).trim();
      if (!new RegExp(`^codeterm ${version.replaceAll('.', '\\.')}$$`, 'i').test(output)) throw new Error(`Embedded CLI version mismatch: ${output}`);
    }
    const summary = `${kind}/${expectedArch}: artifact architecture, source SHA, plugins, resources and version PASS; CLI execution ${canExecute ? 'PASS' : 'SKIPPED (cross-target)'}\n`;
    console.log(summary);
    if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, summary);
  } finally { rmSync(scratch, { recursive: true, force: true }); }
}
if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try { smoke(...process.argv.slice(2)); } catch (error) { console.error(`::error::Bundle structural smoke failed: ${error.message}`); process.exitCode = 1; }
}
