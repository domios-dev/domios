import assert from 'node:assert/strict';
import {test} from 'node:test';
import {createHash} from 'node:crypto';
import {nativePasteReceipt, nativePasteBody} from './records.mjs';
function transfer(body) {
  return {kind:'phaseb_native_paste',paste_id:'paste:fixture',bytes:Buffer.byteLength(body),
    sha256:createHash('sha256').update(body).digest('hex')};
}
test('compact receipt survives command echo while large Unicode body arrives via paste', () => {
  const body = JSON.stringify({session_id:'own',text:'λ界'.repeat(40000)});
  const receipt = transfer(body);
  assert.ok(receipt.bytes > 65536);
  assert.deepEqual(nativePasteReceipt('command echo\n'+JSON.stringify(receipt)+'\n'), receipt);
  assert.equal(nativePasteBody(receipt,{body}),body);
});
test('duplicate or absent native receipt refuses uncertainty', () => {
  const line=JSON.stringify(transfer('{}'));
  assert.throws(()=>nativePasteReceipt('mid-text without receipt'));
  assert.throws(()=>nativePasteReceipt(line+'\n'+line));
});
test('byte length, hash and body envelope are verified before native parsing', () => {
  const body=JSON.stringify({session_id:'own',text:'opaque'});
  const receipt=transfer(body);
  assert.throws(()=>nativePasteBody(receipt,{body:body.slice(1)}));
  assert.throws(()=>nativePasteBody({...receipt,sha256:'0'.repeat(64)},{body}));
  assert.throws(()=>nativePasteBody(receipt,{output:body}));
});
