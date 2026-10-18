import test from 'node:test';
import assert from 'node:assert/strict';
import {Window} from 'happy-dom';
import {installReplayPage} from './replay-page.mjs';

test('controlled frames prove replay harness input/tail acknowledgements and metadata-only capture; not browser latency',async () => {
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
