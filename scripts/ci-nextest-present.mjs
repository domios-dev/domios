#!/usr/bin/env node
// Caller: CI tests.yml (rust test steps)
// Reports whether the runner already has the pinned cargo-nextest, so a job never reinstalls it under a sibling job.
import { spawnSync } from 'node:child_process';
import { appendFileSync } from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

export const hasNextest = (versionOutput, version) => new RegExp(`^release: ${version.replaceAll('.', '[.]')}$`, 'm').test(versionOutput);

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const version = process.argv[2];
  const probe = spawnSync('cargo-nextest', ['nextest', '--version'], { encoding: 'utf8' });
  const present = hasNextest(probe.stdout || '', version);
  console.log(`cargo-nextest ${version} ${present ? 'present; install skipped' : 'missing; installing'}`);
  appendFileSync(process.env.GITHUB_OUTPUT, `present=${present}\n`);
}
