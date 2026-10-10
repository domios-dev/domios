import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {readFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import assert from 'node:assert/strict';
const exec = promisify(execFile);
const wrapper = fileURLToPath(new URL('./dc.sh',import.meta.url));
const nativePath = fileURLToPath(new URL('./native.py',import.meta.url));
export async function dc(args) {
  const {stdout} = await exec('bash',[wrapper,...args],{timeout:30_000,maxBuffer:32*1024*1024});
  const value = JSON.parse(stdout);
  if (value.ok === false) throw new Error('DEV CLI refused: '+JSON.stringify(value.error));
  return value.data ?? value;
}
function binding(value, tab) {
  const record = Array.isArray(value) ? value.find(t=>t.id===tab.tabId) : value;
  assert.equal(record.id ?? record.tab_id,tab.tabId);
  assert.equal(record.agent_session_id,tab.sessionId,'Exact claimed tab session changed');
  return record;
}
export async function snapshot(manifest, machineKey, tab) {
  const remote = machineKey === 'windows' ? ['--mesh',manifest.windows.peerId] : [];
  const before = binding(await dc(['tab','inspect','--tab',tab.tabId,'--json',...remote]),tab);
  const encoded = Buffer.from(JSON.stringify(tab.source)).toString('base64');
  let output;
  if (machineKey === 'mac') {
    output = (await exec('python3',[nativePath,encoded],{timeout:30_000,maxBuffer:32*1024*1024})).stdout;
  } else {
    const code = Buffer.from(await readFile(nativePath)).toString('base64');
    const python = manifest.windows.python;
    assert.match(python,/^(python|python3|py)$/);
    const command = `${python} -c "import base64;exec(base64.b64decode('${code}'))" "${encoded}"`;
    const result = await dc(['tab','exec',command,'--tab',manifest.windows.recordShellTab,
      '--timeout','25000','--json',...remote]);
    assert.equal(result.exit_code,0);
    output = result.output;
  }
  const native = JSON.parse(output);
  assert.equal(native.session_id,tab.sessionId);
  assert.equal(native.provider,tab.provider);
  const after = binding(await dc(['tab','inspect','--tab',tab.tabId,'--json',...remote]),tab);
  return {...native, binding:{before,after}};
}
