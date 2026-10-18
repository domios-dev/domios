import test from 'node:test';
import assert from 'node:assert/strict';
import {distribution,cpuBoundary,summarizeCpu,traceCollector} from './replay-metrics.mjs';
test('bounded distributions retain nearest-rank tails and unknown empty samples',() => {
  const result = distribution(Array.from({length:100},(_,i) => i+1));
  assert.deepEqual(result,{count:100,total:5050,p50:50,p95:95,p99:99,max:100});
  assert.deepEqual(distribution([]),{count:0,total:0,p50:null,p95:null,p99:null,max:null});
});
test('CPU stage attribution preserves unknown self time and never fabricates inclusive durations',() => {
  assert.equal(cpuBoundary({url:'/src/components/MeshPanel/MeshCanvas.tsx'}),'mesh-graph');
  assert.equal(cpuBoundary({url:'/src/components/Terminal/useChatWindowView.ts'}),'projection');
  assert.equal(cpuBoundary({url:'/node_modules/react-dom.js',functionName:'commitRoot'}),'react-commit');
  assert.equal(cpuBoundary({url:'/node_modules/remark-gfm/index.js'}),'markdown');
  const result = summarizeCpu({nodes:[{id:1,callFrame:{functionName:'unknown'}}],samples:[1,1],timeDeltas:[12,19]});
  assert.deepEqual(result.sampledSelfMicroseconds,{other:31});
  assert.throws(() => summarizeCpu({samples:[1,1]},1),/budget/);
});
test('trace totals overlap explicitly, marker timestamps correlate peaks and payloads are discarded',() => {
  const trace = traceCollector();
  trace.ingest([{ph:'I',name:'soak-step-2-3-1024',ts:10,pid:1,tid:1},{ph:'X',name:'Layout',ts:15,dur:30,pid:1,tid:1,args:{private:'secret'}},
    {ph:'B',name:'EventDispatch',ts:20,pid:1,tid:1},{ph:'E',ts:50,pid:1,tid:1},
    {ph:'X',name:'Layout',ts:20,dur:99999,pid:2,tid:2}]);
  const result = trace.result();
  assert.equal(result.totals.Layout.microseconds,30); assert.equal(result.totals.EventDispatch.microseconds,30);
  assert.equal(result.peaks[0].coordinate.messageBytes,1024);
  assert.equal(result.totals.Layout.count,1); assert.equal(result.mainThread,'1:1');
  assert(!JSON.stringify(result).includes('secret'));
  const bounded = traceCollector(1); bounded.ingest([{ph:'X',name:'Layout',ts:1,dur:1},{ph:'X',name:'Layout',ts:2,dur:1}]);
  assert.equal(bounded.result().overflow,true);
  assert.equal(traceCollector(20,{pid:99}).result().totals,null);
});
