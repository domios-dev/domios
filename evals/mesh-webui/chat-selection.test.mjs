import test from 'node:test';
import assert from 'node:assert/strict';
import {readSelectedVisibleComposer} from './chat-selection.mjs';
function mounted(id, shown) {
  const editor={getAttribute:key=>key==='data-tab-id' ? id : null,
    checkVisibility:()=>shown,getClientRects:()=>[{width:shown ? 100 : 0,height:shown ? 30 : 0}]};
  return {getAttribute:key=>key==='data-tab-id' ? id : null,
    checkVisibility:()=>shown,getClientRects:()=>[{width:shown ? 200 : 0,height:shown ? 200 : 0}],
    querySelectorAll:()=>[editor]};
}
test('three mounted editors select only the visible current tab',()=>{
  const roots=[mounted('foreign-one',false),mounted('selected',true),mounted('foreign-two',false)];
  assert.deepEqual(readSelectedVisibleComposer({tabId:'selected',roots}),{tabId:'selected'});
});
test('visible foreign editor cannot receive selected hidden tab input',()=>{
  assert.throws(()=>readSelectedVisibleComposer({tabId:'selected',roots:[mounted('selected',false),mounted('foreign',true)]}));
});
test('duplicate visible selected surfaces refuse before input',()=>{
  assert.throws(()=>readSelectedVisibleComposer({tabId:'selected',roots:[mounted('selected',true),mounted('selected',true)]}));
});
test('editor identity must equal selected visible surface',()=>{
  const root=mounted('selected',true);root.querySelectorAll=()=>mounted('foreign',true).querySelectorAll();
  assert.throws(()=>readSelectedVisibleComposer({tabId:'selected',roots:[root]}));
});
