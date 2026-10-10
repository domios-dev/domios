import assert from 'node:assert/strict';
import {test} from 'node:test';
import {createHash} from 'node:crypto';
import {nativePasteReceipt, nativePasteBody} from './records.mjs';
function transfer(body) {
  return {kind:'phaseb_native_paste',paste_id:'paste:fixture',bytes:Buffer.byteLength(body),
    sha256:createHash('sha256').update(body).digest('hex')};
}
const frame = receipt => `B3R:id=${receipt.paste_id}\nB3R:bytes=${receipt.bytes}\nB3R:sha=${receipt.sha256}\n`;
test('short receipt frames large Unicode body transferred by paste', () => {
  const body=JSON.stringify({session_id:'own',text:'λ界'.repeat(40000)});
  const receipt=transfer(body);
  assert.ok(receipt.bytes>65536);
  assert.ok(frame(receipt).trim().split('\n').every(line=>line.length<80));
  assert.deepEqual(nativePasteReceipt('command echo\n'+frame(receipt)),receipt);
  assert.equal(nativePasteBody(receipt,{body}),body);
});
test('absent, duplicate or malformed receipt fields refuse uncertainty', () => {
  const receipt=transfer('{}'); const text=frame(receipt);
  for (const output of ['',text+text,text.replace('B3R:sha=', 'missing='),text.replace('B3R:bytes=2','B3R:bytes=2x')]) {
    assert.throws(()=>nativePasteReceipt(output));
  }
});
test('byte length, hash and body envelope are verified before parsing', () => {
  const body='opaque';const receipt=transfer(body);
  assert.throws(()=>nativePasteBody(receipt,{body:body.slice(1)}));
  assert.throws(()=>nativePasteBody({...receipt,sha256:'0'.repeat(64)},{body}));
  assert.throws(()=>nativePasteBody(receipt,{output:body}));
});
test('wrapped command echo and ConPTY escapes leave short sentinel fields intact', () => {
  const receipt=transfer('opaque');
  const output='python -c encoded_command_that_wraps\r\nmore_command\x1b[0K\r\n\x1b[?25l'+frame(receipt).replaceAll('\n','\x1b[0m\r\n\x1b[32m')+'\x1b[0mPS C:> ';
  assert.deepEqual(nativePasteReceipt(output),receipt);
  assert.throws(()=>nativePasteReceipt(output+frame(receipt)));
});
