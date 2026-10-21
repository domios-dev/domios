#!/usr/bin/env node
// Caller: CI .github/actions/ci-artifact-spool
// Publishes the name and path of what scripts/ci-artifact.sh spooled while the private store was unreachable.
import { appendFileSync, existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export function spoolOutputs(runnerTemp) {
  const spool = path.join(runnerTemp, 'ci-artifact-spool');
  const marker = path.join(spool, '.name');
  if (!existsSync(marker)) return '';
  return `name=${readFileSync(marker, 'utf8').trim()}\npath=${spool}\n`;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const outputs = spoolOutputs(process.env.RUNNER_TEMP);
  if (outputs) appendFileSync(process.env.GITHUB_OUTPUT, outputs);
}
