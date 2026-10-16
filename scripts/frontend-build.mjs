import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { capture, renderReport } from './collect-rust-warnings.mjs';
import { frontendWarningCollection } from './frontend-warnings.mjs';

export async function buildFrontend({ root = process.cwd(), platform = process.platform, run = capture } = {}) {
  const directory = path.join(root, 'ci-rust-warnings');
  const log = path.join(directory, 'frontend.log');
  fs.mkdirSync(directory, { recursive: true });
  fs.writeFileSync(log, '');
  let exit = 1;
  try {
    exit = await run(process.execPath, [path.join(root, 'node_modules/typescript/bin/tsc')], log);
    if (!exit) exit = await run(process.execPath, [path.join(root, 'node_modules/vite/bin/vite.js'), 'build'], log);
  } catch (error) {
    console.error(error);
    exit = 1;
  }
  const baseline = JSON.parse(fs.readFileSync(path.join(root, 'scripts/rust-warnings-baseline.json'), 'utf8'));
  const collection = frontendWarningCollection(fs.readFileSync(log, 'utf8'), { platform, root, complete: exit === 0 }, baseline);
  fs.writeFileSync(path.join(directory, `rust-warnings-${platform}-frontend.json`), JSON.stringify(collection, null, 2) + '\n');
  fs.writeFileSync(path.join(directory, 'frontend-warnings.md'), renderReport(collection.warnings, ['frontend'], collection.decision).replace('# Rust warnings', '# Frontend warnings'));
  return exit || (collection.decision.added.length ? 1 : 0);
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) process.exitCode = await buildFrontend();
