import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {readFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {exportNative} from './native-node.mjs';
import {nativePasteReceipt,nativePasteBody} from './records.mjs';

const exec=promisify(execFile);
export async function desktopDc(args) {
  const {stdout}=await exec('C:/Program Files/Git/bin/bash.exe',[new URL('./desktop-dc.sh',import.meta.url).pathname.replace(/^\/([A-Za-z]:)/,'$1'),...args],{timeout:30000,maxBuffer:32*1024*1024});
  const value=JSON.parse(stdout);assert.notEqual(value.ok,false);return value.data??value;
}
function binding(value,tab) {
  const record=Array.isArray(value)?value.find(t=>(t.id??t.tab_id)===tab.tabId):value;
  assert.equal(record?.id??record?.tab_id,tab.tabId);
  assert.equal(record.agent_session_id,tab.sessionId,'Exact claimed session changed');
  return record;
}
const quote=value=>"'"+String(value).replaceAll("'","'\\''")+"'";
export async function desktopSnapshot(manifest,machine,tab) {
  assert.notEqual(tab.doNotTouch,true,'Protected native source');
  const peer=machine==='mac'?['--mesh',manifest.mac.peerId]:[];
  const before=binding(await desktopDc(['tab','inspect','--tab',tab.tabId,'--json',...peer]),tab);
  let native;
  if(machine==='windows') native=await exportNative(tab.source);
  else {
    const exporter=Buffer.from(await readFile(new URL('./native-node.mjs',import.meta.url))).toString('base64');
    const spec=Buffer.from(JSON.stringify(tab.source)).toString('base64');
    const program=`import {writeFile,mkdtemp,unlink,rmdir} from 'node:fs/promises';import {tmpdir} from 'node:os';import path from 'node:path';import {execFile} from 'node:child_process';import {promisify} from 'node:util';import {createHash} from 'node:crypto';const m=await import('data:text/javascript;base64,${exporter}');const payload=Buffer.from(JSON.stringify(await m.exportNative(JSON.parse(Buffer.from('${spec}','base64')))));if(payload.length>16777216)throw Error('Native export exceeds16MiB');const folder=await mkdtemp(path.join(tmpdir(),'phaseb-b5-'));const file=path.join(folder,'native.json');await writeFile(file,payload);try{const command='export CODETERM_INSTANCE=aiderqa CODETERM_API_PORT=7697; source /Users/dev/Developer/codeterm-worktrees/rc-mac/evals/_lib/lib.sh; [ "$EVAL_LIB_DEV_API_PORT" = 7697 ] && [ "$EVAL_LIB_RESOLVED_INSTANCE" = aiderqa ] || exit 7; dc paste put --from-file "$1" --pop --ttl 5m --json';const result=await promisify(execFile)('bash',['-lc',command,'b5-native',file],{timeout:20000,maxBuffer:1048576});const p=JSON.parse(result.stdout);console.log('B3R:id='+p.id);console.log('B3R:bytes='+payload.length);console.log('B3R:sha='+createHash('sha256').update(payload).digest('hex'));}finally{await unlink(file);await rmdir(folder);}`;
    const response=await desktopDc(['tab','exec','node --input-type=module -e '+quote(program),'--tab',manifest.mac.recordShellTab,'--timeout','25000','--json',...peer]);
    assert.equal(response.exit_code,0);assert.notEqual(response.output_truncated,true);
    const receipt=nativePasteReceipt(response.output);
    const paste=await desktopDc(['paste','get',receipt.paste_id,'--json',...peer]);
    native=JSON.parse(nativePasteBody(receipt,paste));
  }
  assert.equal(native.session_id,tab.sessionId);assert.equal(native.provider,tab.provider);
  const after=binding(await desktopDc(['tab','inspect','--tab',tab.tabId,'--json',...peer]),tab);
  return {...native,binding:{before,after}};
}
