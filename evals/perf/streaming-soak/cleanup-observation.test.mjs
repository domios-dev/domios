import test from 'node:test';
import assert from 'node:assert/strict';
import {cleanupStateComplete,installCleanupObserverPage} from './cleanup-observation.mjs';
import {ownedChatComposerMounted} from './readiness.mjs';

test('composer readiness is the owned mounted DOM surface',() => {
  const root = tabId => ({dataset:{tabId},querySelector:selector => tabId === 'owned-tab' && selector === 'textarea[aria-label="Message composer"]' ? {} : null});
  const document = {querySelectorAll:() => [root('other-tab'),root('owned-tab')]};
  assert.equal(ownedChatComposerMounted(document,'owned-tab'),true);
  assert.equal(ownedChatComposerMounted(document,'missing-tab'),false);
  assert.equal(ownedChatComposerMounted({querySelectorAll:() => [{dataset:{tabId:'owned-tab'},querySelector:() => null}]},'owned-tab'),false);
});

test('cleanup confirmation requires a shaped snapshot with every owned resource absent',() => {
  const complete = {tabs:{foreign:{}},workspaces:[{id:'foreign'}]};
  assert.equal(cleanupStateComplete(complete,['owned-tab'],'owned-workspace'),true);
  assert.equal(cleanupStateComplete({...complete,tabs:{...complete.tabs,'owned-tab':{}}},['owned-tab'],'owned-workspace'),false);
  assert.equal(cleanupStateComplete({...complete,workspaces:[...complete.workspaces,{id:'owned-workspace'}]},['owned-tab'],'owned-workspace'),false);
  assert.equal(cleanupStateComplete({tabs:{},workspaces:[]},['owned-tab'],'owned-workspace'),true);
  assert.equal(cleanupStateComplete({tabs:{}},['owned-tab'],'owned-workspace'),false);
  assert.equal(cleanupStateComplete(complete,[],'owned-workspace'),false);
});

test('cleanup observer waits for mux change and confirms state without retaining event payloads',async () => {
  const prior = Object.getOwnPropertyDescriptor(globalThis,'window');
  const handlers = new Map(), states = [
    {tabs:{'owned-tab':{}},workspaces:[{id:'owned-workspace'}]},
    {tabs:{foreign:{}},workspaces:[{id:'foreign'}]},
  ];
  let reads = 0, unlistens = 0;
  globalThis.window = {};
  try {
    const bridge = {
      async listen(name,handler) { assert.equal(name,'mux-state-changed'); handlers.set(name,handler); return () => { unlistens++; handlers.delete(name); }; },
      async invoke(name) { assert.equal(name,'mux_get_state'); return states[reads++]; },
    };
    await installCleanupObserverPage({tabs:['owned-tab'],workspace:'owned-workspace'},cleanupStateComplete,async () => bridge);
    assert.equal(reads,0);
    let settled = false;
    const result = window.__streamingSoakCleanup.wait().then(value => { settled = true; return value; });
    handlers.get('mux-state-changed')({payload:{secret:'not retained'}});
    for (let i=0;i<8;i++) await Promise.resolve();
    assert.equal(settled,false);
    assert.equal(reads,1);
    handlers.get('mux-state-changed')({payload:{secret:'not retained'}});
    const receipt = await result;
    assert.deepEqual(receipt,{events:2,reads:2,tabsAbsent:true,workspaceAbsent:true});
    assert.equal(unlistens,1);
    assert(!JSON.stringify(receipt).includes('secret'));
    window.__streamingSoakCleanup.dispose();
    assert.equal(window.__streamingSoakCleanup,undefined);
  } finally {
    if (prior) Object.defineProperty(globalThis,'window',prior); else delete globalThis.window;
  }
});

test('cleanup observer refuses an event stream beyond its finite budget',async () => {
  const prior = Object.getOwnPropertyDescriptor(globalThis,'window');
  let handler, unlistens = 0;
  globalThis.window = {};
  try {
    const bridge = {
      async listen(_name,onChange) { handler = onChange; return () => { unlistens++; }; },
      async invoke() { return {tabs:{'owned-tab':{}},workspaces:[{id:'owned-workspace'}]}; },
    };
    await installCleanupObserverPage({tabs:['owned-tab'],workspace:'owned-workspace',maxEvents:1},cleanupStateComplete,async () => bridge);
    const pending = window.__streamingSoakCleanup.wait();
    handler({payload:{}}); handler({payload:{}});
    await assert.rejects(pending,/event budget exceeded/);
    assert.equal(unlistens,1);
    window.__streamingSoakCleanup.dispose();
  } finally {
    if (prior) Object.defineProperty(globalThis,'window',prior); else delete globalThis.window;
  }
});
