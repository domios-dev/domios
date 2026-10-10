import test from 'node:test';
import assert from 'node:assert/strict';
import {installSelectedActivityObserver} from './activity-observer.mjs';
function root(id,state,shown=true) {
  return {getAttribute:()=>id,checkVisibility:()=>shown,getClientRects:()=>[{width:100,height:100}],
    querySelector:selector=>selector.includes('agent-state') ? {
      getAttribute:key=>key==='data-agent-state' ? state : 'work',textContent:'selected status 2s'
    } : selector.includes('role="log"') ? {textContent:'selected progress'} : null};
}
function observe(roots,tabId) {
  const host={};let mutation;
  class Observer {constructor(callback){mutation=callback;}observe(){}disconnect(){}}
  installSelectedActivityObserver({tabId,doc:{querySelectorAll:()=>roots,documentElement:{}},host,
    Observer,now:()=>10,monotonic:()=>20});
  return {host,mutation};
}
test('activity ignores two foreign mounted roots with nonzero bounds',()=>{
  const {host}=observe([root('foreign-one','idle'),root('selected','working'),root('foreign-two','question')],'selected');
  assert.equal(host.__b2Activity.events[0].tabId,'selected');
  assert.equal(host.__b2Activity.events[0].state,'working');
  assert.equal(host.__b2Activity.events[0].progress,'selected progress');
});
test('missing selected activity never falls back to foreign idle',()=>{
  const {host}=observe([root('foreign','idle')],'selected');
  assert.equal(host.__b2Activity.events[0].available,false);
  assert.equal(host.__b2Activity.events[0].state,undefined);
});
test('mutation appends selected state transition',()=>{
  const roots=[root('selected','working')];const {host,mutation}=observe(roots,'selected');
  roots[0]=root('selected','idle');mutation();
  assert.deepEqual(host.__b2Activity.events.map(e=>e.state),['working','idle']);
});
