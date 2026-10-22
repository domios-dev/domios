import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';

const keys = ['CARGO_BUILD_TARGET', 'CARGO_TARGET_DIR', 'CARGO_BUILD_BUILD_DIR', 'CARGO_INCREMENTAL',
  'RUSTC', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER', 'RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS'];

export function compilerInputs(args, { env = process.env, cwd = process.cwd(), runner = spawnSync } = {}) {
  const manifestIndex = args.indexOf('--manifest-path');
  const manifest = manifestIndex < 0 ? 'Cargo.toml' : args[manifestIndex + 1];
  const result = runner('cargo', ['metadata', '--offline', '--no-deps', '--format-version=1', '--manifest-path', manifest], { cwd, env, encoding: 'utf8' });
  if (result.status !== 0) throw new Error(`compiler-input metadata failed: ${result.error?.message || result.stderr}`);
  const metadata = JSON.parse(result.stdout);
  return { cwd, worktree: env.CT_REAL_WORKTREE ?? cwd, manifest,
    workspace_root: metadata.workspace_root, target_directory: metadata.target_directory, build_directory: metadata.build_directory,
    environment: Object.fromEntries(keys.map(key => [key, env[key] ?? null])) };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const file = path.join(process.env.CT_BUILD_ARTIFACT_DIR, 'compiler-inputs.json');
    fs.writeFileSync(file, `${JSON.stringify(compilerInputs(process.argv.slice(2)), null, 2)}\n`);
    console.log(`compiler-inputs: artifact=${file}`);
  } catch (error) { console.error(`WARNING: compiler-input diagnostics unavailable: ${error.message}`); }
}
