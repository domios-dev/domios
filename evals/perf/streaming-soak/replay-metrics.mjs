export function distribution(values) {
  const sorted = values.filter(Number.isFinite).sort((a,b) => a-b);
  const percentile = p => sorted.length?sorted[Math.max(0,Math.ceil(p*sorted.length)-1)]:null;
  return {count:sorted.length,total:sorted.reduce((sum,n) => sum+n,0),p50:percentile(.5),p95:percentile(.95),p99:percentile(.99),max:sorted.at(-1)??null};
}
export function cpuBoundary(frame) {
  const url = frame.url??'', name = frame.functionName??'';
  if (name === '(idle)') return 'idle';
  if (name === '(garbage collector)') return 'gc';
  if (/replay-page\.mjs/.test(url)) return 'harness';
  if (/MeshPanel|useCanvasViewport/.test(url)) return 'mesh-graph';
  if (/markdown|micromark|mdast|remark|rehype|lowlight|highlight|messageParts/i.test(url)) return 'markdown';
  if (/projectTranscript|transcriptRows|reduceToolEvents/.test(name)) return 'projection';
  if (/transcript(Store)?\.(ts|js)|chat-core.*(?:merge|transcript)|\/merge\.ts/.test(url)) return 'transcript-model';
  if (/useChatWindowView|chatWindow\.|messageTimeline|chatDelivery/.test(url)) return 'projection';
  if (/react[-_].*(?:dom|reconciler)|react-dom/.test(url)) return /commit/i.test(name)?'react-commit':'react-other';
  if (name === 'JSON.parse') return 'json-parse';
  return 'other';
}
export function summarizeCpu(profile,limit=250_000) {
  if (!profile?.samples || profile.samples.length > limit) throw Error('CPU profile sample budget exceeded or unavailable');
  const nodes = new Map(profile.nodes.map(node => [node.id,node])), groups = {};
  const stacks = {};
  for (let index=0;index<profile.samples.length;index++) {
    const frame = nodes.get(profile.samples[index])?.callFrame ?? {};
    const group = cpuBoundary(frame), us = profile.timeDeltas?.[index] ?? 0;
    groups[group] = (groups[group]??0)+us;
    const label = group+':'+(frame.functionName??'').slice(0,120);
    stacks[label] = (stacks[label]??0)+us;
  }
  return {sampledSelfMicroseconds:groups,samples:profile.samples.length,topSelf:Object.entries(stacks).sort((a,b) => b[1]-a[1]).slice(0,20),
    scope:'Statistical leaf samples, not inclusive stage durations. React render/commit may overlap other work. Unrecognized stages remain other.'};
}
const traceNames = new Set(['RunTask','EventDispatch','FunctionCall','Layout','UpdateLayoutTree','Paint','PrePaint','Commit','CompositeLayers','ParseHTML','V8.GCScavenger','V8.GCCompactor']);
export function traceCollector(limit=200_000,{pid=null}={}) {
  let count = 0, overflow = false;
  const threads = new Map(), stacks = new Map(), markers = [];
  function duration(event,microseconds) {
    if (!traceNames.has(event.name) || microseconds < 0) return;
    const key = event.pid+':'+event.tid;
    if (!threads.has(key)) {
      if (threads.size >= 128) { overflow = true; return; }
      threads.set(key,{totals:{},peaks:[]});
    }
    const {totals,peaks} = threads.get(key);
    const current = totals[event.name]??{count:0,microseconds:0,maxMicroseconds:0};
    current.count++; current.microseconds += microseconds; current.maxMicroseconds = Math.max(current.maxMicroseconds,microseconds); totals[event.name] = current;
    peaks.push({name:event.name,microseconds,ts:event.ts}); peaks.sort((a,b) => b.microseconds-a.microseconds); peaks.length = Math.min(peaks.length,20);
  }
  return {
    isOverflow:() => overflow,
    ingest(events) {
      for (const event of events) {
        if (++count > limit) { overflow = true; break; }
        const marker = /^soak-step-(\d+)-(\d+)-(\d+)$/.exec(event.name);
        if (marker && (pid === null || event.pid === pid)) markers.push({ts:event.ts,pid:event.pid,tid:event.tid,segment:Number(marker[1]),step:Number(marker[2]),messageBytes:Number(marker[3])});
        if (event.ph === 'X') duration(event,event.dur??0);
        else if (event.ph === 'B') {
          const key = event.pid+':'+event.tid, stack = stacks.get(key)??[];
          if (stack.length >= 512) { overflow = true; continue; }
          stack.push({name:event.name,ts:event.ts,pid:event.pid,tid:event.tid}); stacks.set(key,stack);
        } else if (event.ph === 'E') {
          const start = stacks.get(event.pid+':'+event.tid)?.pop();
          if (start) duration(start,event.ts-start.ts);
        }
      }
    },
    result() {
      markers.sort((a,b) => a.ts-b.ts);
      const keys = [...new Set(markers.map(marker => marker.pid+':'+marker.tid))];
      const selected = keys.length === 1?threads.get(keys[0]):null;
      return {events:count,overflow,mainThread:keys.length === 1?keys[0]:null,totals:selected?.totals??null,peaks:(selected?.peaks??[]).map(peak => {
        let left=0,right=markers.length;
        while (left < right) { const mid = (left+right)>>>1; if (markers[mid].ts <= peak.ts) left = mid+1; else right = mid; }
        return {...peak,coordinate:markers[left-1]??null};
      }),scope:'Only the renderer PID/thread proven by owned step markers; missing/ambiguous attribution is unknown. Durations overlap, not additive CPU time. A preceding step marker correlates workload, not causation.'};
    },
  };
}
