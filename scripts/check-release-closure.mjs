#!/usr/bin/env node
import { readFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { validateTestWaiver } from './release-test-waiver.mjs';

const PLATFORMS = ['darwin-aarch64', 'darwin-x86_64', 'windows-x86_64',
  'windows-aarch64', 'linux-x86_64', 'linux-aarch64'];

function verifyRecord(record, version, platforms, deb) {
  if (!record || record.verified !== true || record.version !== version || record.cache_busted !== true
    || !Array.isArray(record.failures) || record.failures.length
    || !Array.isArray(record.platforms) || platforms.some(name => !record.platforms.includes(name))) {
    throw new Error('Incomplete published-manifest verification for v' + version);
  }
  const url = new URL(record.manifest);
  const filename = deb ? 'domios-latest-deb.json' : 'domios-latest.json';
  if (url.protocol !== 'https:' || url.host !== 'github.com' || !url.searchParams.has('cb')
    || url.pathname !== `/domios-dev/domios/releases/download/v${version}/${filename}`) {
    throw new Error('Verification must cite this public release manifest');
  }
}

export function authorizeClosure(record, debRecord, version) {
  if (!/^\d+\.\d+\.\d+$/.test(version)) throw new Error('Invalid closure version');
  verifyRecord(record, version, PLATFORMS, false);
  verifyRecord(debRecord, version, ['linux-x86_64', 'linux-aarch64'], true);
  if (record.mode === 'qualified_metadata' || debRecord.mode === 'qualified_metadata') {
    if (record.mode !== 'qualified_metadata' || debRecord.mode !== 'qualified_metadata' ||
        !/^[a-f0-9]{40}$/.test(record.qualification?.source_sha ?? '') ||
        !/^[1-9]\d*$/.test(record.qualification?.run_id ?? '') ||
        JSON.stringify(record.qualification) !== JSON.stringify(debRecord.qualification)) {
      throw new Error('Closure requires the same qualified source and run for both manifests');
    }
    const waiver = record.qualification.test_waiver;
    if (waiver && validateTestWaiver(waiver).run_id !== record.qualification.run_id) throw new Error('Foreign closure waiver');
  }
}

export async function closeVerifiedIssues(plan, record, debRecord, version, close) {
  authorizeClosure(record, debRecord, version);
  if (!Array.isArray(plan) || plan.length > 1000 || new Set(plan.map(action => action.number)).size !== plan.length
    || plan.some(action => !Number.isSafeInteger(action.number) || action.number < 1
    || typeof action.comment !== 'string' || !action.comment.startsWith(`Released in v${version}\n`))) {
    throw new Error('Invalid public issue close plan');
  }
  const waiver = record.qualification?.test_waiver;
  for (const action of plan) await close(waiver ? { ...action,
    comment: action.comment + `\n\nOwner test waiver, qualification run ${waiver.run_id}:\n` + waiver.jobs.map(name => '- ' + name).join('\n'),
  } : action);
}

async function main(argv) {
  const args = {};
  for (let i = 0; i < argv.length; i += 2) {
    if (!['--evidence', '--deb-evidence', '--version', '--plan'].includes(argv[i]) || !argv[i + 1]) {
      throw new Error('Usage: --evidence FILE --deb-evidence FILE --version X.Y.Z [--plan FILE]');
    }
    args[argv[i]] = argv[i + 1];
  }
  const read = async filename => JSON.parse(await readFile(filename, 'utf8'));
  const record = await read(args['--evidence']);
  const debRecord = await read(args['--deb-evidence']);
  const version = args['--version'];
  authorizeClosure(record, debRecord, version);
  if (args['--plan']) {
    await closeVerifiedIssues(await read(args['--plan']), record, debRecord, version, action => {
      execFileSync('gh', ['issue', 'close', String(action.number), '--repo', 'rollacode/codeterm-releases',
        '--comment', action.comment], { stdio: 'inherit', timeout: 60_000 });
    });
  }
  console.log(`Closure authorised for v${version}: all six updater platforms and both Debian platforms verified`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  main(process.argv.slice(2)).catch(error => {
    console.error('refusing to close: ' + error.message);
    process.exitCode = 1;
  });
}
