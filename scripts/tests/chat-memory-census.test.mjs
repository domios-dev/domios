import assert from 'node:assert/strict';
import test from 'node:test';
import {rendererCensus,CENSUS} from '../../evals/perf/chat-memory/census.mjs';

test('census attributes bounded rows and terminal buffers without reading message text',()=>{
  const list=(values)=>({querySelectorAll:selector=>values[selector]??[]});
  const chat={...list({'*':Array(25),'[data-source-message-id]':Array(4)}),getAttribute:()=> 'chat-owned'};
  const terminal={__codetermTerminal:{buffer:{normal:{length:10024},alternate:{length:24},active:{length:24}},cols:80,rows:24},closest:()=>({getAttribute:()=> 'terminal-owned'})};
  const unknown={closest:()=>null};
  const root=list({'.terminal-xterm-host':[terminal,unknown],'[data-testid="tab-session-chat"]':[chat],
    '.xterm':Array(2),'canvas':[{width:100,height:50}],'img':[{naturalWidth:20,naturalHeight:30}],
    '*':Array(42),'[data-source-message-id]':Array(4)});
  const census=rendererCensus(root,{__TAURI_INTERNALS__:{callbacks:new Map([[1,()=>{}]])}});
  assert.deepEqual(census.chats,[{tab:'chat-owned',elements:25,rows:4}]);
  assert.equal(census.terminalApisAvailable,1);
  assert.equal(census.terminals[0].normalLines,10024);
  assert.equal(census.terminals[1].normalLines,null);
  assert.equal(census.canvasBackingPixels,5000);
  assert.equal(census.imageIntrinsicPixels,600);
  assert.equal(census.callbacks,1);
  assert.equal(census.userAgentMemory.available,false);
});

test('in-page census reports a memory API capability without invoking it',()=>{
  let calls=0;
  const browser={crossOriginIsolated:true,performance:{measureUserAgentSpecificMemory:()=>{calls++;}}};
  const result=new Function('document','window',`return ${CENSUS}`)({querySelectorAll:()=>[]},browser);
  assert.equal(result.userAgentMemory.available,true);
  assert.equal(result.userAgentMemory.crossOriginIsolated,true);
  assert.equal(calls,0);
  assert.equal(result.xterms,0);
  assert.deepEqual(result.chats,[]);
});
