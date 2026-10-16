import {test} from 'node:test';
import assert from 'node:assert/strict';
import {aggregateVisitMeasures} from './measures.mjs';

test('visit measures isolate tabs, visit boundaries and request identities',()=>{
  const entry=(name,startTime,duration,tabId='a',requestId='one',extra={})=>({name:'chat_start:'+name,startTime,duration,detail:{tabId,requestId,...extra}});
  const actual=aggregateVisitMeasures([
    entry('response',10,4,'a','one',{responseBytes:1024}),entry('adopted',14,2),
    entry('painted',16,8),entry('total',10,14),entry('response',11,1,'b'),
    entry('total',1,20),entry('total',24,2),entry('response',20,2,'a','two',{responseBytes:0}),
    {...entry('total',10,1),name:'unrelated'},
  ],{tabId:'a',startTime:10,endTime:24});
  assert.equal(actual.measures.length,5);
  assert.deepEqual(actual.requests,[{requestId:'one',responseBytes:1024,stages:{response:4,adopted:2,painted:8,total:14}},
    {requestId:'two',responseBytes:0,stages:{response:2}}]);
});
test('retained visits or release bundles can have no attach measures',()=>{
  assert.deepEqual(aggregateVisitMeasures([],{tabId:'a',startTime:0,endTime:10}),{measures:[],requests:[]});
});
