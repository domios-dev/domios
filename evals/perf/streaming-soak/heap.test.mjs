import test from 'node:test';
import assert from 'node:assert/strict';
import {snapshotGraph,analyzeGraph,heapDiff} from './heap.mjs';
const node = (name,bytes,edges=[]) => ({name,bytes,edges:edges.map(([to,name]) => ({to,name}))});
test('diamond ownership resolves shared dominator and additive self bytes',() => {
  const graph = [node('root',0,[[1,'a'],[2,'b']]),node('Cache',10,[[3,'entry']]),node('Store',20,[[3,'shared']]),node('Row',30)];
  const result = analyzeGraph(graph);
  assert.deepEqual(result.idom,[0,0,0,0]); assert.deepEqual(result.retained,[60,10,20,30]);
  const row = [...result.paths.values()].find(r => r.constructor === 'Row');
  assert.equal(row.retainingPath,'root/a→Cache/entry→Row'); assert.equal(row.dominatorPath,'root');
});
test('cycles terminate, nested retained sizes and unreachable nodes are explicit',() => {
  const result = analyzeGraph([node('root',0,[[1,'a']]),node('Cache',10,[[2,'entry']]),node('Row',30,[[1,'cycle']]),node('orphan',99)]);
  assert.deepEqual(result.idom,[0,0,1,-1]); assert.deepEqual(result.retained,[40,40,30,99]); assert.equal(result.unreachable,1);
});
test('path changes are visible even when constructor totals stay constant',() => {
  const make = edge => analyzeGraph([node('root',0,[[1,edge]]),node('Row',30)]);
  const diff = heapDiff(make('old-cache'),make('new-subscription'));
  assert.equal(diff.constructors.find(r => r.constructor === 'Row').selfBytes,0);
  assert.deepEqual(diff.retainingPaths.filter(r => r.constructor === 'Row').map(r => r.selfBytes).sort((a,b) => a-b),[-30,30]);
});
test('constructor growth and deletion produce signed deltas',() => {
  const make = bytes => analyzeGraph([node('root',0,[[1,'cache']]),node('Row',bytes)]);
  assert.equal(heapDiff(make(10),make(30)).constructors.find(r => r.constructor === 'Row').selfBytes,20);
  assert.equal(heapDiff(make(30),make(10)).constructors.find(r => r.constructor === 'Row').selfBytes,-20);
});
test('a second strong owner changes dominator attribution with the same shortest path',() => {
  const before = analyzeGraph([node('root',0,[[1,'a'],[2,'b']]),node('Cache',10,[[3,'entry']]),node('Store',20),node('Row',30)]);
  const after = analyzeGraph([node('root',0,[[1,'a'],[2,'b']]),node('Cache',10,[[3,'entry']]),node('Store',20,[[3,'shared']]),node('Row',30)]);
  const rows = heapDiff(before,after).retainingPaths.filter(r => r.constructor === 'Row');
  assert.equal(rows.length,2); assert.equal(rows[0].retainingPath,rows[1].retainingPath);
  assert.deepEqual(rows.map(r => r.dominatorPath).sort(),['root','root/a→Cache']);
});
test('V8 metadata positions are decoded and weak retention is excluded',() => {
  const snapshot = {snapshot:{meta:{node_fields:['name','edge_count','self_size'],edge_fields:['name_or_index','to_node','type'],
    edge_types:[[],[],['property','weak','element']]}},strings:['root','Row','held','weak'],
    nodes:[0,2,0,1,0,20,1,0,99],edges:[2,3,0,3,6,1]};
  const graph = snapshotGraph(snapshot);
  assert.equal(graph[0].edges.length,1); assert.equal(graph[0].edges[0].name,'property:held');
  assert.equal(analyzeGraph(graph).unreachable,1);
  assert.throws(() => snapshotGraph({...snapshot,edges:[2,999,0,3,6,1]}));
  assert.throws(() => snapshotGraph({...snapshot,edges:[]}));
});
