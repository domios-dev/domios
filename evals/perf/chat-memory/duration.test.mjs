import test from 'node:test';
import assert from 'node:assert/strict';
import {workloadDuration,continueWorkload,sampleWorkload} from './duration.mjs';

test('duration is bounded and elapsed time ends load without implying readiness', () => {
  assert.equal(workloadDuration(undefined), null);
  assert.equal(workloadDuration('600'), 600000);
  for (const value of ['0', '601', '-1', '1.5', 'bad']) assert.throws(() => workloadDuration(value));
  assert.equal(continueWorkload({duration:600000,started:100,now:600099,cycle:9,cycles:4}), true);
  assert.equal(continueWorkload({duration:600000,started:100,now:600100,cycle:9,cycles:4}), false);
  assert.equal(continueWorkload({duration:600000,started:100,now:200,cycle:10001,cycles:4}), false);
});

test('cycle mode stays finite and timed samples have a fixed minimum interval', () => {
  assert.equal(continueWorkload({duration:null,started:0,now:1,cycle:4,cycles:4}), true);
  assert.equal(continueWorkload({duration:null,started:0,now:1,cycle:5,cycles:4}), false);
  assert.equal(sampleWorkload({duration:600000,now:60099,lastSample:100}), false);
  assert.equal(sampleWorkload({duration:600000,now:60100,lastSample:100}), true);
  assert.equal(sampleWorkload({duration:null,now:101,lastSample:100}), true);
});
