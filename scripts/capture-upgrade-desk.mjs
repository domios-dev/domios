// Caller: skill gates
import {execFileSync} from 'node:child_process';
import {readFileSync} from 'node:fs';
import {validateCapture} from '../evals/upgrade-restore/model.mjs';

const [out, name = 'latest', cli = 'codeterm'] = process.argv.slice(2);
if (!out) throw new Error('Usage: node scripts/capture-upgrade-desk.mjs OUT [TIMESTAMP|latest] [CLI]');
console.error('THIS IS PROD: read-only sanitized desk export through the CLI; no instance state is changed.');
execFileSync(cli, ['sessions', 'backups', 'export', name, '--sanitize', '--out', out],
  {stdio: 'inherit', timeout: 90_000});
const tabs = validateCapture(JSON.parse(readFileSync(out, 'utf8')));
console.log(`Captured ${tabs.length} desk leaves with verified byte sizes.`);
