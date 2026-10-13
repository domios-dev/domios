const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const fixture = JSON.parse(fs.readFileSync(path.join(__dirname,'shared.json'),'utf8'));
const sandbox = vm.createContext({module:{exports:{}}});
vm.runInContext(fs.readFileSync(path.join(__dirname,'plugin.js'),'utf8'),sandbox);
const plugin = sandbox.module.exports;
const plain = value => JSON.parse(JSON.stringify(value));

test('path-loaded generic plans, terminal results and refusals match literal expectations',()=>{
  for (const entry of fixture.operations) {
    const context=entry.context || fixture.context;
    const plan=plugin.runtimePlan(context,entry.operation);
    assert.deepEqual(plain(plan),entry.plan);
    if (entry.reply) assert.deepEqual(plain(plugin.runtimeResponse(context,entry.operation,plan.state,entry.reply)),entry.step);
    else assert.equal(plan.action.kind,"questionAnswer");
  }
});
test('path-loaded ordered frame reduction preserves items and explicit caller state',()=>{
  for (const entry of fixture.reductions) {
    assert.deepEqual(plain(plugin.runtimeReduce(entry.context,entry.state,entry.frames)),entry.expected);
    let state=entry.state;
    const events=[];
    for (const frame of entry.frames) {
      const reduction=plugin.runtimeReduce(entry.context,state,[frame]);
      state=reduction.state;
      events.push(...reduction.events);
    }
    assert.deepEqual(plain({state,events}),entry.expected);
  }
});
