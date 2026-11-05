#!/usr/bin/env bash
# Caller: skill github-release
# Private arrays require one online runner matching every label; hosted images need no private registration.
set -euo pipefail
node --input-type=module <<'JS'
import {execFileSync} from 'node:child_process';
import {sourceRepository} from './scripts/source-repository.mjs';
const repo = sourceRepository();
const api = endpoint => JSON.parse(execFileSync('gh', ['api', endpoint, '--paginate', '--slurp'], {
  encoding: 'utf8', windowsHide: true,
}));
const variables = Object.fromEntries(api(`repos/${repo}/actions/variables`)
  .flatMap(page => page.variables).map(row => [row.name, row.value]));
const defaults = {
  CI_LINUX_LIGHT_RUNNER: 'ubuntu-24.04',
  CI_LINUX_ARM64_RUNNER: 'ubuntu-24.04-arm',
  CI_WINDOWS_RUNNER: 'windows-2025',
  CI_WINDOWS_ARM64_RUNNER: 'windows-11-arm',
  CI_MACOS_RUST_RUNNER: 'macos-15',
  CI_MACOS_EMBEDDINGS_RUNNER: 'macos-15',
  CI_LINUX_RUST_FULL_RUNNER: 'ubuntu-24.04',
  CI_RELEASE_EVIDENCE_RUNNER: 'ubuntu-24.04',
  RELEASE_MACOS_ARM64_RUNNER: 'macos-15',
  RELEASE_MACOS_X64_RUNNER: 'macos-15-intel',
  RELEASE_LINUX_X64_RUNNER: 'ubuntu-24.04',
  RELEASE_WINDOWS_X64_RUNNER: 'windows-2025',
  RELEASE_WINDOWS_ARM64_COMPILE_RUNNER: 'windows-2025',
};
const selectors = Object.entries(defaults).map(([key, fallback]) => {
  const raw = variables[key];
  const value = !raw || key === 'CI_LINUX_RUST_FULL_RUNNER' && raw === 'hosted'
    ? fallback : JSON.parse(raw);
  if (typeof value === 'string' && /^(ubuntu-|macos-|windows-)[A-Za-z0-9.-]+$/.test(value)) {
    console.log(`hosted   ${key}: ${value} (verify availability through Actions)`);
  } else if (!Array.isArray(value) || value.length < 2 || !value.includes('self-hosted') ||
    !value.every(label => typeof label === 'string' && label.length)) {
    throw new Error(`Invalid ${key}: use a hosted image as a JSON string or a self-hosted label array`);
  }
  return {key, value};
});
const privateSelectors = selectors.filter(row => Array.isArray(row.value));
if (privateSelectors.length) {
  const runners = [...api(`repos/${repo}/actions/runners`), ...api(`orgs/${repo.split('/')[0]}/actions/runners`)]
    .flatMap(page => page.runners);
  for (const {key, value} of privateSelectors) {
    const online = runners.filter(runner => runner.status === 'online' && value.every(label =>
      runner.labels.some(actual => actual.name.toLowerCase() === label.toLowerCase())));
    if (online.length) console.log(`online   ${key}: ${[...new Set(online.map(row => row.name))].join(', ')}`);
    else {
      console.error(`OFFLINE  ${key}: no online runner matches ${JSON.stringify(value)}`);
      process.exitCode = 1;
    }
  }
}
JS
