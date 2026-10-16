import test from 'node:test';
import assert from 'node:assert/strict';
import {fixtureRecord,growth} from '../../evals/perf/chat-memory/fixture.mjs';

test('synthetic records carry exact session identity and fresh row ids',()=>{
  const input={session:'fixture-session',cwd:'/owned/fixture',run:'run',index:0};
  const first=fixtureRecord(input),second=fixtureRecord({...input,index:1,parent:first.uuid});
  const row=JSON.parse(second.line);
  assert.equal(row.sessionId,input.session);
  assert.equal(row.parentUuid,first.uuid);
  assert.notEqual(first.uuid,second.uuid);
  assert.ok(row.message.content[0].text.includes(second.marker));
  assert.ok(Buffer.byteLength(second.line)<8192);
});

test('memory deltas never join a reused process id',()=>{
  assert.deepEqual(growth([{pid:1,created:'old',privateBytes:100}],[{pid:1,created:'new',privateBytes:900}]),[]);
  assert.equal(growth([{pid:1,created:'same',privateBytes:100}],[{pid:1,created:'same',privateBytes:140}])[0].delta,40);
});
