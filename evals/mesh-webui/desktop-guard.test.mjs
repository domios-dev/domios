import test from 'node:test';
import assert from 'node:assert/strict';
import {desktopTarget,refuseDesktopLifecycle,desktopOutput} from './desktop-guard.mjs';
import {parseNativeJsonl,nativeBlocks} from './native-node.mjs';
const dev={type:'page',url:'http://127.0.0.1:1420/',id:'DEV',webSocketDebuggerUrl:'ws://127.0.0.1:9223/devtools/page/DEV'};
test('exact one DEV page is selected before connection',()=>assert.equal(desktopTarget([dev]).id,'DEV'));
test('another window or wrong port refuses',()=>{
  assert.throws(()=>desktopTarget([dev,{...dev,id:'OTHER'}]));
  assert.throws(()=>desktopTarget([{...dev,url:'http://127.0.0.1:7685/'}]));
  assert.throws(()=>desktopTarget([{...dev,webSocketDebuggerUrl:'ws://127.0.0.1:9222/devtools/page/DEV'}]));
});
test('desktop lifecycle methods refuse without delegating',async()=>{
  const context={},page={context:()=>context};refuseDesktopLifecycle(page);
  for(const method of ['goto','reload','close','setContent','goBack','goForward']) await assert.rejects(page[method]());
  await assert.rejects(context.close());await assert.rejects(context.newPage());
});
test('evidence output stays inside approved direct root',()=>{
  assert.equal(desktopOutput('/tmp/fixture/b5-123','/tmp/fixture'),'/tmp/fixture/b5-123');
  assert.throws(()=>desktopOutput('/tmp/foreign/b5-123','/tmp/fixture'));
});
test('native Codex joins only exact session and preserves markers',()=>{
  const rows=[[1,{type:'session_meta',payload:{id:'S'}}],[2,{type:'turn_context',payload:{model:'M'}}],[3,{type:'response_item',payload:{role:'user',content:[{text:'OPAQUE'}]}}],[4,{type:'event_msg',payload:{type:'turn_aborted'}}]];
  const value=parseNativeJsonl('codex','S','owned.jsonl',rows);
  assert.equal(value.records[2].text,'OPAQUE');assert.equal(value.records[2].model,'M');assert.equal(value.records[3].cancelled,true);
  assert.throws(()=>parseNativeJsonl('codex','FOREIGN','owned.jsonl',rows));
});
test('native Claude omits foreign and sidechain rows',()=>{
  const value=parseNativeJsonl('claude','S','owned.jsonl',[[1,{sessionId:'S',type:'assistant',message:{model:'M',content:[{text:'OPAQUE'}]}}],[2,{sessionId:'FOREIGN'}],[3,{sessionId:'S',isSidechain:true}]]);
  assert.equal(value.records.length,1);assert.equal(value.records[0].text,'OPAQUE');
});
test('native Grok model and cancellation stay session-scoped',()=>{
  const value=parseNativeJsonl('grok','S','owned.jsonl',[[1,{params:{update:{sessionUpdate:'user_message_chunk',content:{text:'OPAQUE'},_meta:{modelId:'M'}}}}],[2,{params:{update:{sessionUpdate:'prompt_response',stopReason:'cancelled'}}}]]);
  assert.equal(value.records[0].model,'M');assert.equal(value.records[1].cancelled,true);
});
test('native text blocks preserve supplied opaque payload',()=>assert.equal(nativeBlocks([{type:'text',text:'A'},{type:'text',text:'B'}]),'AB'));
