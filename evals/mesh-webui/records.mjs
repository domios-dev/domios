import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {readFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
const exec = promisify(execFile);
const wrapper = fileURLToPath(new URL('./dc.sh',import.meta.url));
const nativePath = fileURLToPath(new URL('./native.py',import.meta.url));
export async function dc(args) {
  const {stdout} = await exec('bash',[wrapper,...args],{timeout:30_000,maxBuffer:32*1024*1024});
  const value = JSON.parse(stdout);
  if (value.ok === false) throw new Error('DEV CLI refused: '+JSON.stringify(value.error));
  return value.data ?? value;
}
const MAX_EXPORT_BYTES = 16 * 1024 * 1024;
export function nativePasteReceipt(output) {
  const receipts = String(output).split(/\r?\n/).flatMap(line => {
    try { const value = JSON.parse(line); return value.kind === 'phaseb_native_paste' ? [value] : []; }
    catch { return []; }
  });
  assert.equal(receipts.length, 1, 'Exactly one native paste receipt required');
  const receipt = receipts[0];
  assert.match(receipt.paste_id, /^paste:[a-zA-Z0-9_-]+$/);
  assert.match(receipt.sha256, /^[a-f0-9]{64}$/);
  assert.ok(Number.isSafeInteger(receipt.bytes) && receipt.bytes > 0 && receipt.bytes <= MAX_EXPORT_BYTES);
  return receipt;
}
export function nativePasteBody(receipt, value) {
  assert.equal(typeof value.body, 'string', 'Native paste requires exact JSON body');
  const payload = Buffer.from(value.body, 'utf8');
  assert.equal(payload.length, receipt.bytes, 'Native paste byte length changed');
  assert.equal(createHash('sha256').update(payload).digest('hex'), receipt.sha256, 'Native paste hash changed');
  return value.body;
}
function windowsNativePasteCommand(code, spec, python) {
  assert.match(python, /^(python|python3|py)$/);
  const bridge = `set -euo pipefail
unset CODETERM_INSTANCE
export CODETERM_API_PORT=7686
source /c/Developer/codeterm/codeterm/evals/_lib/lib.sh
[ "$EVAL_LIB_DEV_API_PORT" = 7686 ] && [ -z "$EVAL_LIB_RESOLVED_INSTANCE" ] || exit 7
dc paste put --from-file "$1" --pop --ttl 5m --content-type application/json --json`;
  const wrapper = `import base64,json,pathlib,subprocess
namespace={'__name__':'phaseb_native_export'}
exec(base64.b64decode(${JSON.stringify(code)}),namespace)
spec=json.loads(base64.b64decode(${JSON.stringify(spec)}))
transfer=namespace['export_to_file'](spec)
result=subprocess.run(['C:/Program Files/Git/bin/bash.exe','-lc',${JSON.stringify(bridge)},'phaseb-native-export',transfer['path']],capture_output=True,text=True,check=True,timeout=20)
paste=json.loads(result.stdout)
print(json.dumps(dict(kind='phaseb_native_paste',paste_id=paste['id'],bytes=transfer['bytes'],sha256=transfer['sha256'])))
path=pathlib.Path(transfer['path'])
path.unlink()
path.parent.rmdir()`;
  const encoded = Buffer.from(wrapper).toString('base64');
  return `${python} -c "import base64;exec(base64.b64decode('${encoded}'))"`;
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
    const command = windowsNativePasteCommand(code, encoded, manifest.windows.python);
    const result = await dc(['tab','exec',command,'--tab',manifest.windows.recordShellTab,
      '--timeout','25000','--json',...remote]);
    assert.equal(result.exit_code,0);
    assert.notEqual(result.output_truncated, true, 'Compact native receipt was truncated');
    const receipt = nativePasteReceipt(result.output);
    const paste = await dc(['paste','get',receipt.paste_id,'--json',...remote]);
    output = nativePasteBody(receipt, paste);
  }
  const native = JSON.parse(output);
  assert.equal(native.session_id,tab.sessionId);
  assert.equal(native.provider,tab.provider);
  const after = binding(await dc(['tab','inspect','--tab',tab.tabId,'--json',...remote]),tab);
  return {...native, binding:{before,after}};
}
