import { readFileSync } from 'node:fs';
import { dirname, resolve, relative, isAbsolute } from 'node:path';
import { pathToFileURL } from 'node:url';
import { recoveryVerdict, diagnosticVerdict, sha256 } from './evidence-model.mjs';

const hash = value => /^[a-f0-9]{64}$/.test(value ?? '');
const asset = value => value?.url && hash(value.sha256) && Number.isInteger(value.bytes) && value.bytes > 0 && value.origin;
const buffer = value => value && typeof value.text === 'string' && typeof value.type === 'string' && Number.isInteger(value.length) &&
  Number.isInteger(value.baseY) && Number.isInteger(value.viewportY);
const selectedFixture = value => typeof value?.tabId === 'string' && value.tabId.length > 0 && value.authority &&
  ['agent_session_id', 'session_binding_nonce', 'pty_id', 'boot_stage'].every(key => Object.hasOwn(value.authority, key));
export function evidenceVerdict(value) {
  if (value?.schema !== 1 || !value.scenario) return 'incomplete';
  if (value.scenario === 'phase-origins') {
    const required = ['cli_begin', 'socket_received', 'socket_dispatched', 'socket_bound', 'utility_created_at', 'PtyStarting', 'AgentReady'];
    return value.status === 'captured' && value.rows?.length && value.rows.every(row => row.tabId && required.every(name =>
      Number.isFinite(row.origins?.[name]?.value) && row.origins[name].source && row.origins[name].unit)) ? 'captured' : 'incomplete';
  }
  if (value.status !== 'completed' || !value.binarySha || !value.listener?.pid || !value.listener?.started_us) return 'incomplete';
  if (value.mode === 'dry-run') {
    if (value.scenario === 'browser-evidence') return value.target?.id && value.target.url && selectedFixture(value.fixture) ? 'validated' : 'incomplete';
    if (value.scenario === 'exact-session-recovery') return value.before?.userIds?.length && value.before.process?.created ? 'validated' : 'incomplete';
    return 'incomplete';
  }
  if (value.finalListener?.pid !== value.listener.pid || value.finalListener?.started_us !== value.listener.started_us) return 'incomplete';
  if (value.scenario === 'exact-session-recovery') return recoveryVerdict(value.before, value.after).status;
  if (value.scenario !== 'browser-evidence') return 'incomplete';
  if (!selectedFixture(value.fixture) || value.failures?.length || !asset(value.index) || !value.assets?.length || !value.assets.every(record => asset(record.loaded) && asset(record.served)) ||
      !value.streams?.length || !value.streams.every(stream => stream.status === 200 && stream.artifact?.file && hash(stream.artifact.sha256) && stream.artifact.bytes > 0) ||
      !value.after?.terminals?.length || value.after.terminals.some(terminal => !terminal.tabId || !buffer(terminal.active) || !buffer(terminal.normal) || !buffer(terminal.alternate) || !terminal.ancestors?.length))
    return 'incomplete';
  const diagnostic = diagnosticVerdict(value.diagnostics ?? []).status;
  return diagnostic === 'pass' ? 'captured' : diagnostic;
}

export function verifyByteArtifacts(receipt, directory, read = readFileSync) {
  if (receipt.scenario !== 'browser-evidence' || receipt.mode === 'dry-run') return;
  const required = [];
  if (receipt.index) required.push({ ...receipt.index, file: `served-index-${receipt.index.sha256}.html` });
  for (const record of receipt.assets ?? []) {
    required.push({ ...record.loaded, file: `loaded-${record.loaded.sha256}.js` });
    required.push({ ...record.served, file: `served-${record.served.sha256}.js` });
  }
  for (const stream of receipt.streams ?? []) if (stream.artifact) required.push(stream.artifact);
  for (const artifact of required) {
    const path = resolve(directory, artifact.file), rel = relative(resolve(directory), path);
    if (rel.startsWith('..') || isAbsolute(rel)) throw Error('Artifact escapes receipt directory');
    const bytes = read(path);
    if (bytes.length !== artifact.bytes || sha256(bytes) !== artifact.sha256) throw Error('Artifact hash or byte length mismatch');
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const receipt = JSON.parse(readFileSync(process.argv[2], 'utf8').replace(/^\uFEFF/, ''));
  let verdict = evidenceVerdict(receipt);
  try { verifyByteArtifacts(receipt, dirname(resolve(process.argv[2]))); }
  catch { verdict = 'incomplete'; }
  console.log(verdict);
  process.exitCode = verdict === 'incomplete' ? 1 : verdict === 'fail' ? 3 : 0;
}
