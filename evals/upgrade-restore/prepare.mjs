import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {resolve, join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {requireBackings, leaves, measurements, synthesizeHistory, capturedFailedLaunch} from './model.mjs';
import {peer} from './fake-peer.mjs';
import {utilityKind} from './utility.mjs';

const [input, outputRoot] = process.argv.slice(2);
if (!input || !outputRoot) throw new Error('Usage: node prepare.mjs CAPTURE OUTPUT_ROOT');
const document = JSON.parse(readFileSync(input, 'utf8'));
requireBackings(document);
peer(document);
const root = resolve(outputRoot);
const shellPath = fileURLToPath(new URL('./fake-shell.mjs', import.meta.url));
const claudePath = fileURLToPath(new URL('./fake-claude.mjs', import.meta.url));
const shellTrace = join(root, 'shell-trace.jsonl');
const claudeTrace = join(root, 'claude-trace.jsonl');
const claudeConfig = join(root, 'claude-config');
document.replay_utilities = [];
mkdirSync(join(root, 'bin'), {recursive: true});
mkdirSync(join(root, 'project'), {recursive: true});
mkdirSync(claudeConfig, {recursive: true});
for (const workspace of document.snapshot.workspaces) {
  workspace.cwd = join(root, 'project');
  delete workspace.provisioned_cwd;
}
for (const tab of leaves(document)) {
  const failed = capturedFailedLaunch(document, tab);
  const kind = failed ? null : utilityKind(tab);
  if (kind) document.replay_utilities.push({tab_id: tab.tab_id, kind, captured_cwd: tab.cwd,
    captured_shell_profile: tab.shell_profile, captured_surface: tab.surface});
  tab.cwd = join(root, 'project');
  if (tab.agent_identity) delete tab.agent_identity.provisioned_cwd;
  if (kind === 'pty') {
    tab.shell_profile = {id: 'upgrade-restore-fixture', name: 'Fixture', executable: process.execPath,
      args: [shellPath, shellTrace, tab.tab_id], wsl_distro: null};
  } else if (kind === 'surface') {
    tab.surface = {plugin_id: 'upgrade-restore-fixture', surface_id: 'fixture', instance_id: tab.tab_id};
  }
  if (!failed && tab.agent_type === 'claude') {
    const sessionId = tab.claude_session_id ?? tab.agent_session_id;
    const project = join(claudeConfig, 'projects', tab.cwd.replace(/[^A-Za-z0-9]/g, '-'));
    mkdirSync(project, {recursive: true});
    const history = document.histories.find(row => row.tab_id === tab.tab_id);
    const rows = synthesizeHistory(history.shape).map((message, index) => ({
      type: message.type === 'user' ? 'user' : 'assistant',
      uuid: message.id,
      cwd: tab.cwd,
      sessionId,
      timestamp: new Date(index).toISOString(),
      isMeta: message.type === 'thinking',
      message: {role: message.type === 'user' ? 'user' : 'assistant', content: message.content || 'x'},
    }));
    writeFileSync(join(project, `${sessionId}.jsonl`), rows.map(row => JSON.stringify(row)).join('\n') + '\n');
  }
}
document.measurements = measurements(document.snapshot, document.histories, document.measurements);
const fixturePath = join(root, 'desk.json');
const tracePath = join(root, 'provider-trace.jsonl');
writeFileSync(fixturePath, JSON.stringify(document, null, 2));
writeFileSync(tracePath, '');
writeFileSync(shellTrace, '');
writeFileSync(claudeTrace, '');
const peerPath = fileURLToPath(new URL('./fake-peer.mjs', import.meta.url));
const quote = value => `'${value.replaceAll("'", "'\\''")}'`;
const command = [process.execPath, peerPath, fixturePath, tracePath].map(quote).join(' ');
writeFileSync(join(root, 'bin', 'codex'), `#!/bin/sh\nexec ${command} "$@"\n`, {mode: 0o755});
const windowsValues = [process.execPath, peerPath, fixturePath, tracePath];
if (windowsValues.some(value => /[%"\r\n]/.test(value))) throw new Error('Unsafe Windows fixture launcher path');
writeFileSync(join(root, 'bin', 'codex.cmd'), `@echo off\r\n${windowsValues.map(value => `"${value}"`).join(' ')} %*\r\n`);
const claudeWindowsValues = [process.execPath, claudePath, claudeTrace];
if (claudeWindowsValues.some(value => /[%"\r\n]/.test(value))) throw new Error('Unsafe Windows fixture launcher path');
writeFileSync(join(root, 'bin', 'claude'), `#!/bin/sh\nexec ${[process.execPath, claudePath, claudeTrace].map(quote).join(' ')} "$@"\n`, {mode: 0o755});
writeFileSync(join(root, 'bin', 'claude.cmd'), `@echo off\r\n${claudeWindowsValues.map(value => `"${value}"`).join(' ')} %*\r\n`);
console.log(`Prepared ${fixturePath}; start named DEV with ${join(root, 'bin')} first in PATH and CLAUDE_CONFIG_DIR=${claudeConfig}.`);
