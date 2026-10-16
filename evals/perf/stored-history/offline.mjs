import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import { performance } from "node:perf_hooks";

const [rollout, session, beforeBundle, afterBundle, output] = process.argv.slice(2);
if (!output) throw Error("Usage: offline.mjs ROLLOUT SESSION BEFORE_BUNDLE AFTER_BUNDLE OUTPUT");
const budget = 1024 * 1024;
const fd = fs.openSync(rollout, "r");
const windows = [];
try {
  let end = fs.fstatSync(fd).size;
  for (let page = 0; page < 40 && end > 0; page++) {
    const start = Math.max(0, end - budget);
    const bytes = Buffer.alloc(end - start);
    fs.readSync(fd, bytes, 0, bytes.length, start);
    const first = start === 0 ? 0 : bytes.indexOf(10) + 1;
    if (first === 0 && start !== 0) throw Error("Window contains no complete record");
    windows.push({start:start + first, end, records:bytes.subarray(first).toString("utf8")});
    end = start + first;
  }
} finally { fs.closeSync(fd); }
assert.equal(windows.length, 40);
function projector(bundle) {
  const context = vm.createContext({module:{exports:{}}, exports:{}, host:{}});
  new vm.Script(fs.readFileSync(bundle, "utf8")).runInContext(context);
  const nativeParse = vm.runInContext("JSON.parse", context);
  let parsed = [];
  context.countedParse = value => { parsed.push(value); return nativeParse(value); };
  vm.runInContext("JSON.parse = countedParse", context);
  const project = context.module.exports.default.runtimeStoredHistory;
  return window => {
    parsed = [];
    const started = performance.now();
    const rows = project(session, window.records);
    const executeMs = performance.now() - started;
    return {start:window.start,end:window.end,executeMs,
      parsedBytes:parsed.reduce((sum,value)=>sum+Buffer.byteLength(value),0),parsedLines:parsed.length,
      rows:JSON.parse(JSON.stringify(rows))};
  };
}
const projectors = [projector(beforeBundle),projector(afterBundle)];
const results = [[],[]];
for (const [index,window] of windows.entries()) {
  const pair = [];
  for (const side of index % 2 === 0 ? [0,1] : [1,0]) pair[side] = projectors[side](window);
  assert.deepEqual(pair[1].rows,pair[0].rows);
  for (const side of [0,1]) {
    pair[side].rowCount=pair[side].rows.length;
    delete pair[side].rows;
    results[side].push(pair[side]);
  }
}
function summarize(pages) {
  const times = pages.map(page => page.executeMs).sort((a,b) => a-b);
  return {runtime:"Node vm built plugin (not QuickJS)",meanMs:times.reduce((a,b)=>a+b,0)/times.length,
    p95Ms:times[Math.ceil(times.length * .95)-1],parsedBytes:pages.reduce((sum,page)=>sum+page.parsedBytes,0),
    parsedLines:pages.reduce((sum,page)=>sum+page.parsedLines,0),pages};
}
const [before,after] = results.map(summarize);
fs.writeFileSync(output,JSON.stringify({budget,pageCount:windows.length,alternatingOrder:true,identicalRows:true,before,after},null,2));
console.log(JSON.stringify({before:{...before,pages:undefined},after:{...after,pages:undefined},identicalRows:true}));
