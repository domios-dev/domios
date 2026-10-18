import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {execFileSync,spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {prepareReplay,validateReplay,parseExport,chunkEnds,prefixRow,ownedRows,assertComplete,compareReplays,outsideRepository,hash} from './replay-model.mjs';
const exported = () => ({pending:false,count:4,messages:[
  {id:'u',type:'user',content:'opaque user text',seq:20,sourceSessionId:'original'},
  {id:'call',type:'tool_call',content:'',toolId:'linked',toolInput:{command:'opaque command'},toolName:'shell'},
  {id:'result',type:'tool_result',content:'opaque result',toolId:'linked',toolCallSourceId:'call'},
  {id:'answer',type:'assistant',content:'## Heading\n\n```js\nconst value = "🙂";\n```\n\n**Final tail é**',timestamp:'100',sourceSessionId:'original'},
]});
const text = () => JSON.stringify(exported());

test('saved export preserves selected response/tool data, remaps identity and declares reconstructed cadence',() => {
  const replay = prepareReplay(text(),{chunkBytes:13});
  assert.equal(validateReplay(replay),replay);
  assert.equal(replay.historyRows,3); assert.equal(replay.omittedOlder,0);
  assert.match(replay.reconstruction,/unavailable/);
  assert.equal(replay.rows.at(-1).content,exported().messages.at(-1).content);
  assert.deepEqual(replay.rows[1].toolInput,{command:'opaque command'});
  assert.equal(replay.rows[2].toolCallSourceId,replay.rows[1].id);
  assert.equal(replay.rows[0].sourceSessionId,undefined);
  assert.equal(replay.rows[0].seq,20); assert.equal(replay.rows.at(-1).seq,undefined);
  const rows = ownedRows(replay,'run','new-session');
  assert(rows.every(row => row.id.startsWith('soak-run-') && row.sourceSessionId === 'new-session'));
  assert.equal(rows[2].toolCallSourceId,rows[1].id);
  assert.equal(new Set(replay.ends.map(end => prefixRow(rows.at(-1),end).id)).size,1);
});
test('byte-bounded chunks preserve Unicode and exactly finish the saved assistant body',() => {
  const source = 'ab🙂é\n'.repeat(9), ends = chunkEnds(source,7);
  let start = 0;
  for (const end of ends) { const chunk = source.slice(start,end); assert(Buffer.byteLength(chunk) <= 7); assert(!chunk.includes('\uFFFD')); start = end; }
  assert.equal(start,source.length);
  const row = {content:'abcd🙂',segments:[{type:'text',text:'abcd'},{type:'text',text:'🙂'}]};
  assert.equal(prefixRow(row,4).segments.map(s => s.text).join(''),'abcd');
  assert.deepEqual(prefixRow(row,row.content.length),row);
});
test('incomplete/pending exports, opaque envelopes, budgets and fixture tampering refuse without content disclosure',() => {
  for (const patch of [{pending:true},{count:5},{messages:[]}]) assert.throws(() => parseExport(JSON.stringify({...exported(),...patch})));
  assert.throws(() => parseExport('{"private-secret":broken'),error => !error.message.includes('private-secret'));
  assert.throws(() => prepareReplay(text(),{historyRows:401}));
  assert.throws(() => prepareReplay(text(),{chunkBytes:3}));
  assert.throws(() => chunkEnds('x'.repeat(10000),4),/event budget/);
  const envelope = exported(); envelope.messages.at(-1).segments = [{type:'frame',body:'opaque'}];
  assert.throws(() => prepareReplay(JSON.stringify(envelope)),/losslessly/);
  const replay = prepareReplay(text()); replay.rows.at(-1).content += 'changed';
  assert.throws(() => validateReplay(replay),/fingerprint/);
});
test('history variants retain exact selected suffix and declare older rows omitted',() => {
  const replay = prepareReplay(text(),{historyRows:1});
  assert.equal(replay.omittedOlder,2); assert.equal(replay.rows.length,2);
  assert.equal(replay.rows[0].content,exported().messages[2].content);
  assert.notEqual(replay.fingerprint,prepareReplay(text(),{historyRows:2}).fingerprint);
});
test('completeness requires exact own authority, every selected row and final tail contents',() => {
  const rows = ownedRows(prepareReplay(text()),'owned','s'), authority = {session:'s',nonce:'n'};
  const actual = {authority:{sourceSessionId:'s',sessionBindingNonce:'n'},rows,evictedOlder:0};
  assert.equal(assertComplete(rows,actual,authority).tailHash,hash(rows.at(-1)));
  for (const replacement of [rows.slice(1),[...rows.slice(0,-1),{...rows.at(-1),content:rows.at(-1).content.slice(0,-1)}]])
    assert.throws(() => assertComplete(rows,{...actual,rows:replacement},authority),/incomplete/);
  assert.throws(() => assertComplete(rows,actual,{...authority,nonce:'foreign'}),/authority/);
});
test('before/after comparison refuses changed input budgets, controls, surface or incomplete results',() => {
  const before = {complete:true,surface:'native',identity:{build:'before'},workload:{fingerprint:'same',mode:'stream',segments:3},segments:[]};
  assert.equal(compareReplays(before,{...before,identity:{build:'after'}}).after.identity.build,'after');
  for (const patch of [{complete:false},{surface:'chrome'},{workload:{...before.workload,segments:4}},{workload:{...before.workload,mode:'quiet'}}])
    assert.throws(() => compareReplays(before,{...before,...patch}));
});
test('offline CLI prepares private fixture without DEV, refuses repository output, never prints saved text',() => {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(),'soak-replay-test-'));
  const runner = fileURLToPath(new URL('./run.mjs',import.meta.url));
  const root = fileURLToPath(new URL('../../../',import.meta.url));
  try {
    const source = path.join(temp,'source.json'); fs.writeFileSync(source,text());
    const output = execFileSync(process.execPath,[runner,'prepare-replay','--source',source,'--out',path.join(temp,'prepared'),'--chunk-bytes','32'],{encoding:'utf8',env:{...process.env,CODETERM_API_PORT:'',CODETERM_DEV_DATA_DIR:''}});
    assert.match(output,/complete/); assert(!output.includes('opaque user text'));
    assert.equal(validateReplay(JSON.parse(fs.readFileSync(path.join(temp,'prepared','replay-fixture.json'),'utf8'))).historyRows,3);
    assert.throws(() => outsideRepository(path.join(root,'private.json'),root));
    const refused = spawnSync(process.execPath,[runner,'prepare-replay','--source',source,'--out',root],{encoding:'utf8'});
    assert.notEqual(refused.status,0); assert(!fs.existsSync(path.join(root,'replay-fixture.json')));
  } finally { fs.rmSync(temp,{recursive:true}); }
});
