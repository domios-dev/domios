import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {resolve, join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {requireBackings, leaves, measurements} from './model.mjs';
import {peer} from './fake-peer.mjs';

const [input, outputRoot] = process.argv.slice(2);
if (!input || !outputRoot) throw new Error('Usage: node prepare.mjs CAPTURE OUTPUT_ROOT');
const document = JSON.parse(readFileSync(input, 'utf8'));
requireBackings(document);
peer(document);
const root = resolve(outputRoot);
mkdirSync(join(root, 'bin'), {recursive: true});
mkdirSync(join(root, 'project'), {recursive: true});
for (const workspace of document.snapshot.workspaces) {
  workspace.cwd = join(root, 'project');
  delete workspace.provisioned_cwd;
}
for (const tab of leaves(document)) {
  tab.cwd = join(root, 'project');
  if (tab.agent_identity) delete tab.agent_identity.provisioned_cwd;
}
document.measurements = measurements(document.snapshot, document.histories);
const fixturePath = join(root, 'desk.json');
const tracePath = join(root, 'provider-trace.jsonl');
writeFileSync(fixturePath, JSON.stringify(document, null, 2));
writeFileSync(tracePath, '');
const peerPath = fileURLToPath(new URL('./fake-peer.mjs', import.meta.url));
const quote = value => `'${value.replaceAll("'", "'\\''")}'`;
const command = [process.execPath, peerPath, fixturePath, tracePath].map(quote).join(' ');
writeFileSync(join(root, 'bin', 'codex'), `#!/bin/sh\nexec ${command} "$@"\n`, {mode: 0o755});
const windowsValues = [process.execPath, peerPath, fixturePath, tracePath];
if (windowsValues.some(value => /[%"\r\n]/.test(value))) throw new Error('Unsafe Windows fixture launcher path');
writeFileSync(join(root, 'bin', 'codex.cmd'), `@echo off\r\n${windowsValues.map(value => `"${value}"`).join(' ')} %*\r\n`);
console.log(`Prepared ${fixturePath}; start named DEV with ${join(root, 'bin')} first in PATH.`);
