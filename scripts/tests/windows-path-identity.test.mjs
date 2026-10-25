import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import ts from 'typescript';

test('pnpm workspace aliases under a mapped checkout retain one TypeScript global identity', t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'domios-path-identity-'));
  let drive;
  t.after(() => {
    if (drive) execFileSync('subst', [drive, '/D']);
    fs.rmSync(root, { recursive: true, force: true });
  });
  const sdk = path.join(root, 'packages/plugin-sdk');
  const modules = path.join(root, 'node_modules/@codeterm');
  fs.mkdirSync(sdk, { recursive: true });
  fs.mkdirSync(modules, { recursive: true });
  fs.writeFileSync(path.join(sdk, 'package.json'), JSON.stringify({ name: '@codeterm/plugin-sdk', types: 'src/index.ts' }));
  fs.mkdirSync(path.join(sdk, 'src'));
  fs.writeFileSync(path.join(sdk, 'src/index.ts'), 'export {}; declare global { const pathIdentityOracle: unique symbol; }');
  fs.symlinkSync(sdk, path.join(modules, 'plugin-sdk'), process.platform === 'win32' ? 'junction' : 'dir');
  fs.writeFileSync(path.join(root, 'consumer.ts'), 'import "@codeterm/plugin-sdk"; export const proof = pathIdentityOracle;');
  let mapped = root;
  if (process.platform === 'win32') {
    drive = ['Z:', 'Y:', 'W:', 'V:', 'U:'].find(value => !fs.existsSync(`${value}/`));
    assert.ok(drive, 'a private drive mapping is required');
    execFileSync('subst', [drive, root]);
    mapped = `${drive}/`;
  }
  const config = JSON.parse(fs.readFileSync(new URL('../../tsconfig.json', import.meta.url), 'utf8'));
  const options = {
    noEmit: true, types: [], skipLibCheck: true, target: ts.ScriptTarget.ES2021,
    module: ts.ModuleKind.ESNext, moduleResolution: ts.ModuleResolutionKind.Bundler,
    preserveSymlinks: config.compilerOptions.preserveSymlinks ?? false,
    baseUrl: mapped, paths: config.compilerOptions.paths,
  };
  const files = ['consumer.ts', 'packages/plugin-sdk/src/index.ts'].map(file => path.join(mapped, file));
  const diagnostics = settings => ts.getPreEmitDiagnostics(ts.createProgram(files, settings));
  const actual = diagnostics(options);
  assert.deepEqual(actual.map(item => ts.flattenDiagnosticMessageText(item.messageText, '\n')), []);
  const broken = diagnostics({ ...options, paths: undefined, preserveSymlinks: true });
  assert.ok(broken.some(item => item.code === 2451), 'the duplicate-identity control must reproduce TS2451');
});
