#!/usr/bin/env node
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { bashInterpreter } from './ci-runner-paths.mjs';
import { fileURLToPath } from 'node:url';

export function warningArtifactCommand(operation, env = process.env, host = process.platform, discoverBash = bashInterpreter) {
  const file = operation === 'collect'
    ? `rust-warnings-${env.WARNING_PLATFORM}-${env.WARNING_TARGET}.json` : env.WARNING_FILE;
  if (!/^rust-warnings-[A-Za-z0-9_.-]+\.json$/.test(file ?? '')) throw new Error('invalid warning artifact filename');
  const output = `ci-rust-warnings/${file}`;
  if (operation === 'collect') return { program: process.execPath, args: ['scripts/collect-rust-warnings.mjs',
    'collect', '--input', env.WARNING_INPUT, '--platform', env.WARNING_PLATFORM, '--target', env.WARNING_TARGET,
    '--complete', env.WARNING_COMPLETE, '--output', output] };
  if (operation !== 'store' || !/^[1-9]\d*$/.test(env.GITHUB_RUN_ID ?? '')) throw new Error('invalid warning artifact operation/run');
  return { program: discoverBash(host, env),
    args: ['scripts/ci-artifact.sh', 'put', `${env.GITHUB_RUN_ID}/rust-warnings/${file}`, output] };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const command = warningArtifactCommand(process.argv[2]);
    execFileSync(command.program, command.args, { stdio: 'inherit', timeout: 300000 });
  } catch (error) { console.error('warning artifact failed: ' + error.message); process.exitCode = 1; }
}
