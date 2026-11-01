import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { capture, renderReport } from './collect-rust-warnings.mjs';
import { frontendWarningCollection } from './frontend-warnings.mjs';
import { syncBuildDirectory } from './build-artifacts.cjs';

export async function buildFrontend({ root = process.cwd(), platform = process.platform, run = capture } = {}) {
  const directory = path.join(root, 'ci-rust-warnings');
  const log = path.join(directory, 'frontend.log');
  fs.mkdirSync(directory, { recursive: true });
  fs.writeFileSync(log, '');
  const cache = path.join(root, 'node_modules/.cache');
  fs.mkdirSync(cache, { recursive: true });
  let exit = 1;
  let staging;
  try {
    exit = await run(process.execPath, [path.join(root, 'node_modules/typescript/bin/tsc'), '--incremental', '--tsBuildInfoFile', path.join(cache, 'codeterm-types.tsbuildinfo')], log);
    if (!exit) {
      staging = fs.mkdtempSync(path.join(cache, 'codeterm-frontend-'));
      exit = await run(process.execPath, [path.join(root, 'node_modules/vite/bin/vite.js'), 'build', '--emptyOutDir', '--outDir', staging], log);
    }
  } catch (error) {
    console.error(error);
    exit = 1;
  }
  const baseline = JSON.parse(fs.readFileSync(path.join(root, 'scripts/frontend-warnings-baseline.json'), 'utf8'));
  const collection = frontendWarningCollection(fs.readFileSync(log, 'utf8'), { platform, root, complete: exit === 0 }, baseline);
  try {
    if (!exit) syncBuildDirectory(staging, path.join(root, 'dist'));
  } catch (error) {
    console.error(error);
    exit = 1;
    collection.complete = false;
  } finally {
    if (staging) fs.rmSync(staging, { recursive: true, force: true });
  }
  fs.writeFileSync(path.join(directory, `rust-warnings-${platform}-frontend.json`), JSON.stringify(collection, null, 2) + '\n');
  fs.writeFileSync(path.join(directory, 'frontend-warnings.md'), renderReport(collection.warnings, ['frontend'], collection.decision).replace('# Rust warnings', '# Frontend warnings'));
  if (collection.decision.added.length) console.warn(`Frontend warnings advisory: ${collection.decision.added.length} new warning(s); details: ${path.join(directory, 'frontend-warnings.md')}`);
  return exit;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) process.exitCode = await buildFrontend();
