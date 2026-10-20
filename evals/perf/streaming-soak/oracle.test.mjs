import test from 'node:test';
import assert from 'node:assert/strict';
import {METRICS,median,seriesStats,declareControl,plateau} from './oracle.mjs';
const trace = (slope=0) => Array.from({length:61},(_,i) => ({at:i*60_000,postGC:true,...Object.fromEntries(METRICS.map(k => [k,10_000+slope*i/60]))}));
const match = {build:'sha',entryHash:'hash',platform:'test'};
const control = samples => ({version:1,mode:'idle',run:'idle-1',completedAt:4_000_000,match,samples});
test('medians and per-hour slope have physical units',() => {
  assert.equal(median([]),null); assert.equal(median([9,1,3,7]),5);
  const stats = seriesStats(trace(600),'jsBytes');
  assert.ok(Math.abs(stats.perHour-600)<1e-8);
  assert.equal(stats.early20MinuteMedian,10_095); assert.equal(stats.late20MinuteMedian,10_505);
  assert.ok(Math.abs(stats.medianPerHour-600)<1e-8);
});
test('synthetic flat passes; rising fails against predeclared flat idle',() => {
  const declaration = declareControl(control(trace()),match,5_000_000);
  assert.equal(plateau(trace(),declaration).status,'flat');
  assert.equal(plateau(trace(600),declaration).status,'rising');
});
test('allowable slope is derived from matching idle, including declining idle',() => {
  const declaration = declareControl(control(trace(300)),match,5_000_000);
  assert.ok(Math.abs(declaration.allowances.jsBytes-300)<1e-8);
  assert.equal(plateau(trace(200),declaration).status,'flat');
  assert.equal(plateau(trace(400),declaration).status,'rising');
  assert.equal(declareControl(control(trace(-300)),match,5_000_000).allowances.jsBytes,0);
});
test('mismatched, late, load or incomplete controls are refused',() => {
  for (const patch of [{version:2},{mode:'load'},{match:{build:'other'}},{completedAt:6_000_000},{samples:trace().slice(0,40)},{run:null}])
    assert.throws(() => declareControl({...control(trace()),...patch},match,5_000_000));
});
test('missing counters, uncollected GC, gaps and insufficient windows are unknown',() => {
  const declaration = declareControl(control(trace()),match,5_000_000);
  for (const patch of [{jsBytes:null},{jsBytes:undefined},{jsBytes:NaN},{jsBytes:-1},{postGC:false},{at:100_000_000}]) {
    const samples = trace(); Object.assign(samples[30],patch);
    assert.equal(plateau(samples,declaration).status,'unknown');
  }
  assert.equal(plateau(trace().slice(0,40),declaration).status,'unknown');
  assert.equal(plateau(trace(),null).status,'unknown');
  assert.equal(plateau(trace().filter((_,i) => i !== 30),declaration).status,'unknown');
  const samples = trace(); samples[1].at = samples[0].at;
  assert.equal(seriesStats(samples,'jsBytes'),null);
});
