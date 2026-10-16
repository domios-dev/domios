import assert from 'node:assert/strict';
import test from 'node:test';
import {
 createTranscriptState, reduceTranscript, transcriptRows,
 transcriptCommittedEnd, transcriptBackfillCursor,
 type TranscriptState, type TranscriptBatch,
} from './transcript';

const authority={sourceSessionId:'own-session',sessionBindingNonce:'own-binding'};
const row=(index:number,revision=0)=>({id:'row-'+index,type:'assistant' as const,content:index+':'+revision});
const batch=(kind:TranscriptBatch['kind'],start:number,end:number,from:number,to:number,revision=0):TranscriptBatch=>({
 kind,authority,range:{start,end,unit:'bytes'},rows:Array.from({length:to-from},(_,at)=>row(from+at,revision)),
});
const base=()=>reduceTranscript(createTranscriptState(),batch('snapshot',20,50,2,5)).state;
function assertRows(state:TranscriptState,ids:number[],revision=0){
 const rows=transcriptRows(state);
 assert.deepEqual(rows.map(r=>r.id),ids.map(i=>'row-'+i));
 assert.equal(new Set(rows.map(r=>r.id)).size,rows.length);
 for(const entry of rows)assert.equal(entry.content,entry.id.slice(4)+':'+revision);
}
test('record-aligned overlap merges new rows and incoming revisions without restarting',()=>{
 const result=reduceTranscript(base(),batch('delta',40,70,4,7,1));
 assert.equal(result.outcome,'adopted');
 assert.deepEqual(transcriptRows(result.state).map(r=>r.content),['2:0','3:0','4:1','5:1','6:1']);
 assert.equal(transcriptCommittedEnd(result.state),70);
 assert.equal(transcriptBackfillCursor(result.state),20);
});
test('overlap beginning below the loaded window keeps incoming revision precedence',()=>{
 const result=reduceTranscript(base(),batch('delta',10,70,1,7,1));
 assert.equal(result.outcome,'adopted');
 assertRows(result.state,[1,2,3,4,5,6],1);
 assert.equal(transcriptBackfillCursor(result.state),10);
 assert.equal(transcriptCommittedEnd(result.state),70);
});
test('stale below-window data is ignored and a real forward hole requests recovery',()=>{
 const initial=base();
 const stale=reduceTranscript(initial,batch('delta',0,40,0,4,2));
 assert.equal(stale.outcome,'ignored_stale');
 assert.deepEqual(stale.state,initial);
 const gap=reduceTranscript(initial,batch('delta',60,70,6,7));
 assert.equal(gap.outcome,'recover_gap');
 assert.deepEqual(gap.state,initial);
});

test('overlapping equal-text records with distinct source ids remain distinct',()=>{
 const initial=reduceTranscript(createTranscriptState(),{
  kind:'snapshot',authority,range:{start:0,end:20},
  rows:[{id:'first',type:'assistant',content:'same'}],
 }).state;
 const result=reduceTranscript(initial,{
  kind:'delta',authority,range:{start:10,end:30},
  rows:[{id:'second',type:'assistant',content:'same'}],
 });
 assert.equal(result.outcome,'adopted');
 assert.deepEqual(transcriptRows(result.state).map(row=>row.id),['first','second']);
});
test('overlap replay is idempotent across varied record-aligned windows',()=>{
 let seed=17;
 const random=(max:number)=>{seed=(seed*1664525+1013904223)>>>0;return seed%max;};
 for(let round=0;round<100;round++){
  const start=2+random(8),end=start+3+random(8),deltaStart=random(end),deltaEnd=end+1+random(5);
  const initial=reduceTranscript(createTranscriptState(),batch('snapshot',start*10,end*10,start,end)).state;
  const incoming=batch('delta',deltaStart*10,deltaEnd*10,deltaStart,deltaEnd,1);
  const first=reduceTranscript(initial,incoming);
  assert.equal(first.outcome,'adopted');
  const second=reduceTranscript(first.state,incoming);
  assert.equal(second.outcome,'ignored_stale');
  assert.deepEqual(transcriptRows(second.state),transcriptRows(first.state));
  assert.equal(new Set(transcriptRows(first.state).map(r=>r.id)).size,transcriptRows(first.state).length);
 }
});
test('repeated overlapping tails remain bounded and keep the latest shared revisions',()=>{
 let state=reduceTranscript(createTranscriptState(),batch('snapshot',0,10,0,1)).state;
 const latest=new Map<string,string>();
 for(let index=1;index<=1000;index++){
  const from=Math.max(0,index-2);
  const incoming=batch('delta',from*10,(index+1)*10,from,index+1,index);
  for(const row of incoming.rows)latest.set(row.id,row.content);
  const result=reduceTranscript(state,incoming);
  assert.equal(result.outcome,'adopted');
  state=result.state;
 }
 const rows=transcriptRows(state);
 assert.ok(rows.length<=440,'overlap growth must not retain the whole source');
 assert.equal(new Set(rows.map(r=>r.id)).size,rows.length);
 for(const row of rows)assert.equal(row.content,latest.get(row.id));
 assert.equal(transcriptCommittedEnd(state),10010);
 assert.ok((transcriptBackfillCursor(state)??0)>0);
});
test('evicting the oldest overlap advances paging to the retained segment',()=>{
 const initial=reduceTranscript(createTranscriptState(),batch('snapshot',0,30,0,3),3).state;
 const result=reduceTranscript(initial,batch('delta',20,50,2,5,1),3);
 assert.equal(result.outcome,'adopted');
 assertRows(result.state,[2,3,4],1);
 assert.equal(transcriptBackfillCursor(result.state),20);
 assert.equal(transcriptCommittedEnd(result.state),50);
});

test('repeated overlap revisions bound superseded range metadata',()=>{
 let state=reduceTranscript(createTranscriptState(),batch('snapshot',0,20,0,1)).state;
 for(let index=1;index<=1000;index++){
  state=reduceTranscript(state,batch('delta',10,20+index,0,1,index)).state;
 }
 assert.equal(transcriptRows(state).length,1);
 assert.equal(transcriptRows(state)[0].content,'0:1000');
 assert.ok(state.segments.length<=2);
 assert.equal(transcriptBackfillCursor(state),0);
 assert.equal(transcriptCommittedEnd(state),1020);
});

test('older backfill cannot replace a newer delta revision',()=>{
 const initial=reduceTranscript(createTranscriptState(),batch('snapshot',20,40,2,4)).state;
 const live=reduceTranscript(initial,batch('delta',30,50,3,5,2)).state;
 const page=batch('backfill',0,20,0,2,1);
 page.rows.push(row(3,1));
 const result=reduceTranscript(live,page);
 assert.equal(result.outcome,'adopted');
 assert.deepEqual(transcriptRows(result.state).map(row=>row.id),['row-0','row-1','row-2','row-3','row-4']);
 assert.equal(transcriptRows(result.state).find(row=>row.id==='row-3')?.content,'3:2');
 assert.equal(transcriptBackfillCursor(result.state),0);
 assert.equal(transcriptCommittedEnd(result.state),50);
});
