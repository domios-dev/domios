import test from 'node:test';
import assert from 'node:assert/strict';
import {installReplayPage,functionalReplayPrerequisites,ownedBrowserSelection} from './replay-page.mjs';

test('controlled frames prove replay harness input/tail acknowledgements and metadata-only capture; not browser latency',async () => {
  const {Window} = await import('happy-dom');
  const window = new Window(), originals = new Map(), frames = [];
  const globals = {window,document:window.document,HTMLTextAreaElement:window.HTMLTextAreaElement,InputEvent:window.InputEvent,
    EventTarget:window.EventTarget,CustomEvent:window.CustomEvent,MutationObserver:window.MutationObserver,
    PerformanceObserver:class { static supportedEntryTypes = []; },requestAnimationFrame:callback => { frames.push(callback); return frames.length; }};
  for (const [key,value] of Object.entries(globals)) { originals.set(key,Object.getOwnPropertyDescriptor(globalThis,key)); Object.defineProperty(globalThis,key,{value,configurable:true,writable:true}); }
  window.document.body.innerHTML = '<section data-testid="tab-session-chat" data-tab-id="local"><textarea aria-label="Message composer"></textarea></section>';
  const data = new Map([
    ['local:s',{authority:{sourceSessionId:'s',sessionBindingNonce:'n'},rows:[],evictedOlder:0}],
    ['other:o',{authority:{sourceSessionId:'o',sessionBindingNonce:'p'},rows:[],evictedOlder:0}],
  ]);
  const painted = new Map(), monitor = {recordPaint(tab,tail) { painted.set(tab,tail); },snapshot:tab => ({paintedTail:painted.get(tab)})};
  const originalPaint = monitor.recordPaint;
  const modules = {
    '/src/components/Terminal/chatTestBridge.ts':{isCodeTermFrontendTestMode:() => true},
    '/src/components/Terminal/transcriptStore.ts':{transcriptWindowKey:(tab,session) => tab+':'+session,readTranscriptWindow:key => ({transcript:data.get(key)})},
    '/packages/chat-core/src/transcript.ts':{transcriptRows:transcript => transcript.rows},
    '/src/components/Terminal/chatProjectionDiagnostics.ts':{chatProjectionMonitor:monitor},
  };
  window.addEventListener('codeterm:test-chat-reply',event => {
    const {tabId,messages,sourceSessionId,sessionBindingNonce} = event.detail;
    const state = data.get(tabId+':'+sourceSessionId);
    assert.equal(sessionBindingNonce,state.authority.sessionBindingNonce);
    const byId = new Map(state.rows.map(row => [row.id,row]));
    for (const row of messages) byId.set(row.id,row);
    state.rows = [...byId.values()];
    requestAnimationFrame(() => monitor.recordPaint(tabId,messages.at(-1).id+'#'+messages.at(-1).content.length));
  });
  async function flush(promise) {
    let complete = false, failure;
    promise.then(() => { complete = true; },error => { complete = true; failure = error; });
    for (let i=0;i<20 && !complete;i++) {
      for (const callback of frames.splice(0)) callback();
      await Promise.resolve();
    }
    assert(complete,'Controlled event/frame budget exhausted'); if (failure) throw failure;
    return promise;
  }
  const row = {id:'soak-run-tail',sourceSessionId:'s',seq:0,type:'assistant',content:'private response payload'};
  try {
    await installReplayPage({tab:'local',session:'s',nonce:'n',run:'run',other:{tab:'other',session:'o',nonce:'p'},muxTab:'terminal'},async key => modules[key]);
    const page = window.__streamingSoakReplay;
    await flush(page.seed(JSON.stringify([row])));
    page.begin(); page.arm({segment:1,step:1,messageBytes:row.content.length,historyRows:0});
    const step = page.step(JSON.stringify([{...row,content:row.content+'!'}]),'stream');
    const textarea = window.document.querySelector('textarea'); textarea.value = 'x';
    const input = new window.InputEvent('input',{bubbles:true,inputType:'insertText'});
    Object.defineProperty(input,'isTrusted',{value:true}); textarea.dispatchEvent(input);
    await flush(step); await flush(page.acknowledge());
    const report = page.end();
    assert.equal(report.values.captureToDocumentBubble.length,1); assert.equal(report.values.inputToSecondFrame.length,1);
    assert.equal(report.values.parse.length,1); assert.equal(report.inputActions,1);
    assert.equal(report.hasEventTiming,false); assert.equal(report.hasLongTask,false);
    assert(!JSON.stringify(report).includes('private response payload'));
    assert(report.timeline.some(item => item.kind === 'normalized-arrival-store-return' && item.id === row.id && item.characters === row.content.length+1));
    assert.equal(page.read().rows[0].content,row.content+'!');
    await flush(page.seed(JSON.stringify([{...row,id:'soak-run-other-tail',sourceSessionId:'o'}]),true));
    assert.equal(textarea.value,'x'); assert.equal(page.readOther().rows[0].content,row.content);
    page.dispose(); assert.equal(monitor.recordPaint,originalPaint); assert.equal(window.__streamingSoakReplay,undefined);
  } finally {
    window.__streamingSoakReplay?.dispose();
    for (const [key,descriptor] of originals) { if (descriptor) Object.defineProperty(globalThis,key,descriptor); else delete globalThis[key]; }
    await window.happyDOM.abort();
  }
});


test('bundled functional preflight refuses missing mode or shipped handles before setup',() => {
  const original = Object.getOwnPropertyDescriptor(globalThis,'window');
  try {
    const window = {location:{href:'http://127.0.0.1:7686/'}};
    Object.defineProperty(globalThis,'window',{value:window,configurable:true,writable:true});
    assert.deepEqual(functionalReplayPrerequisites(),{available:false,reason:'existing_e2e_entry_required'});
    window.location.href += '?e2e';
    assert.deepEqual(functionalReplayPrerequisites(),{available:false,reason:'bundled_chat_handles_unavailable_open_existing_chat'});
    window.__codetermTestChat = {reply() {},pushMessages() {}};
    window.__chatProjection = {snapshot() {},recordPaint() {}};
    assert.deepEqual(functionalReplayPrerequisites(),{available:true,internalStore:'unavailable',durableStorage:'unavailable',authority:'backend_binding_only'});
  } finally {
    if (original) Object.defineProperty(globalThis,'window',original); else delete globalThis.window;
  }
});

test('bundled functional adapter uses existing public projection without source imports or fake store rows',async () => {
  const originals = new Map();
  const document = new EventTarget();
  const textarea = {value:''};
  let selected = 'local';
  const root = id => ({querySelector:() => textarea,querySelectorAll:() => selected === id ? [{dataset:{sourceMessageId:'soak-run-tail'}}] : []});
  document.querySelector = selector => selector.includes('tab-session-chat')?root(selector.includes('other')?'other':'local'):null;
  const recordPaint = () => {};
  const monitor = {recordPaint,snapshot:() => ({paintedTail:'soak-run-tail#4'})};
  const window = {location:{href:'http://127.0.0.1:7686/?e2e'},__codetermTestChat:{reply() {},pushMessages() {}},__chatProjection:monitor};
  for (const [key,value] of Object.entries({window,document})) {
    originals.set(key,Object.getOwnPropertyDescriptor(globalThis,key));
    Object.defineProperty(globalThis,key,{value,configurable:true,writable:true});
  }
  let imports = 0;
  try {
    const result = await installReplayPage({functional:true,tab:'local',session:'s',nonce:'n',run:'run',other:{tab:'other',session:'o',nonce:'p'}},async () => { imports++; throw Error('Source import forbidden'); });
    assert.equal(imports,0);
    assert.equal(result.internalStore,'unavailable');
    const observation = window.__streamingSoakReplay.read();
    assert.equal(observation.rows,undefined);
    assert.equal(observation.authority,'backend_binding_only');
    assert.equal(observation.durableStorage,'unavailable');
    assert.deepEqual(observation.visibleSourceIds,['soak-run-tail']);
    assert.deepEqual(window.__streamingSoakReplay.readOther().visibleSourceIds,[]);
    selected='other';
    assert.deepEqual(window.__streamingSoakReplay.readOther().visibleSourceIds,['soak-run-tail']);
    assert.deepEqual(window.__streamingSoakReplay.read().visibleSourceIds,[]);
    window.__streamingSoakReplay.dispose();
    assert.equal(monitor.recordPaint,recordPaint);
    assert.equal(window.__streamingSoakReplay,undefined);
  } finally {
    window.__streamingSoakReplay?.dispose();
    for (const [key,descriptor] of originals) {
      if (descriptor) Object.defineProperty(globalThis,key,descriptor); else delete globalThis[key];
    }
  }
});

test('owned Browser selection clicks only exact workspace and tab and gates mounted target',() => {
  let selected = false, active = false, mounted = false, workspaceClicks = 0, tabClicks = 0;
  const node = (dataset={}) => ({dataset,closest:()=>null,getClientRects:()=>[{}],contains:()=>false});
  const button = (label,click,current=()=>null) => ({...node(),getAttribute:key=>key==='aria-label'?label:current(),click});
  const workspaceButton = button('Select workspace fixture',()=>{ selected=true; workspaceClicks++; });
  const tabButton = button('Open tab fixture',()=>{active=true;tabClicks++;},()=>active?'page':null);
  const row = {...node({tabId:'owned-tab'}),querySelectorAll:()=>[tabButton]};
  const detail = {...node({workspaceId:'owned-workspace'}),querySelectorAll:()=>[row]};
  const card = {...node({workspaceId:'owned-workspace'}),querySelectorAll:()=>[workspaceButton]};
  const root = {...node({tabId:'owned-tab'}),querySelector:()=>mounted?{}:null};
  const document = {querySelectorAll:selector=>selector.includes('workspace-detail')?(selected?[detail]:[]):selector.includes('workspace-card')?[card]:selector==='[data-tab-id]'?[root]:[]};
  ownedBrowserSelection(document,'owned-workspace','owned-tab','workspace');
  assert.equal(workspaceClicks,1);
  assert.equal(ownedBrowserSelection(document,'owned-workspace','owned-tab','mounted'),false);
  ownedBrowserSelection(document,'owned-workspace','owned-tab','tab');
  assert.equal(tabClicks,1);
  assert.equal(ownedBrowserSelection(document,'owned-workspace','owned-tab','mounted'),false);
  mounted=true;
  assert.equal(ownedBrowserSelection(document,'owned-workspace','owned-tab','mounted'),true);
  assert.throws(()=>ownedBrowserSelection(document,'owned-workspace','foreign-tab','tab'),/missing or ambiguous/);
  detail.querySelectorAll=()=>[row,row];
  assert.throws(()=>ownedBrowserSelection(document,'owned-workspace','owned-tab','tab'),/ambiguous/);
  assert.equal(tabClicks,1);
  assert.throws(()=>ownedBrowserSelection(document,'foreign-workspace','owned-tab','workspace'),/missing or ambiguous/);
});

test('owned Browser overflow reveals once and waits exact control without arbitrary clicks',() => {
  let expanded=false, present=false, expansions=0, selections=0, duplicates=false;
  const visible = {closest:()=>null,getClientRects:()=>[{}]};
  const activation={...visible,getAttribute:()=> 'Select workspace fixture',click:()=>{selections++;}};
  const row={...visible,dataset:{workspaceId:'owned'},querySelectorAll:()=>[activation]};
  const overflow={...visible,getAttribute:()=>expanded?'true':'false',click:()=>{expanded=true;expansions++;}};
  const document={querySelectorAll:selector=>selector.includes('workspace-card-more')?[overflow]:selector.includes('workspace-card')&&present?(duplicates?[row,row]:[row]):[]};
  ownedBrowserSelection(document,'owned','tab','workspace-reveal');
  assert.equal(expansions,1);
  assert.equal(selections,0);
  assert.equal(ownedBrowserSelection(document,'owned','tab','workspace-ready'),false);
  assert.throws(()=>ownedBrowserSelection(document,'owned','tab','workspace-reveal'),/missing after expansion/);
  assert.throws(()=>ownedBrowserSelection(document,'owned','tab','workspace'),/missing or ambiguous/);
  assert.equal(expansions,1);
  present=true;
  assert.equal(ownedBrowserSelection(document,'owned','tab','workspace-ready'),true);
  ownedBrowserSelection(document,'owned','tab','workspace-reveal');
  assert.equal(expansions,1);
  ownedBrowserSelection(document,'owned','tab','workspace');
  assert.equal(selections,1);
  duplicates=true;
  assert.throws(()=>ownedBrowserSelection(document,'owned','tab','workspace-ready'),/ambiguous/);
  assert.throws(()=>ownedBrowserSelection(document,'owned','tab','workspace-reveal'),/ambiguous/);
  assert.equal(expansions,1);
  assert.equal(selections,1);
});
