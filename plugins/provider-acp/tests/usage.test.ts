import assert from "node:assert/strict";
import {contextUsage,usage} from "../src/runtimeReducePlan";

assert.deepEqual(contextUsage("own",{used:53000,size:200000}),{kind:"context",threadId:"own",used_tokens:53000,window_tokens:200000});
assert.deepEqual(contextUsage("own",{used:0,size:0,cost:{amount:0,currency:"USD"}}),{kind:"context",threadId:"own",used_tokens:0,window_tokens:0,cost_micros:0});
assert.deepEqual(contextUsage("own",{used:1,size:2,cost:{amount:0.0123456,currency:"USD"}}),{kind:"context",threadId:"own",used_tokens:1,window_tokens:2,cost_micros:12346});
for (const cost of [undefined,null,{}, {amount:1}, {amount:1,currency:"EUR"}, {amount:-1,currency:"USD"}, {amount:Infinity,currency:"USD"}, {amount:NaN,currency:"USD"}, {amount:Number.MAX_VALUE,currency:"USD"}]) {
  const event=contextUsage("own",{used:1,size:2,cost});
  assert.ok(event);
  assert.equal("cost_micros" in event,false,"unreported or invalid USD cost is omitted");
}
for (const raw of [{}, {used:1}, {size:2}, {used:null,size:2}, {used:-1,size:2}, {used:1,size:-2}, {used:1.5,size:2}, {used:1,size:Infinity}, {used:Number.MAX_SAFE_INTEGER+1,size:2}]) {
  assert.equal(contextUsage("own",raw),undefined,"invalid or missing context counters do not emit an event");
}
assert.equal(usage({used:1,size:2}),undefined,"context-only observations never fabricate prompt token usage");
assert.deepEqual(usage({inputTokens:2,outputTokens:3,cachedReadTokens:1,cost:{amount:0.001,currency:"USD"}}),{input_tokens:2,output_tokens:3,cache_read_tokens:1,cost_micros:1000});
assert.deepEqual(usage({inputTokens:0,outputTokens:0}),{input_tokens:0,output_tokens:0});
console.log("✓ ACP context-only usage retains observed counters and USD cost; prompt usage remains separate");
