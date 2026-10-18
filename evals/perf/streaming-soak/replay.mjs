import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {evaluate} from '../renderer-memory/cdp.mjs';
import {atomic,readJson} from './io.mjs';
import {visit,sample} from './runtime.mjs';
import {muxBindings,requireValue,requireMeasurement} from './model.mjs';
import {outsideRepository,validateReplay,ownedRows,prefixRow,assertComplete,REPLAY_LIMITS} from './replay-model.mjs';
import {distribution,summarizeCpu,traceCollector} from './replay-metrics.mjs';
import {installReplayPage} from './replay-page.mjs';

export const repositoryRoot = fileURLToPath(new URL('../../../',import.meta.url));
export async function runReplay(rt,cp,out,flags,save) {
  outsideRepository(out,repositoryRoot);
  requireValue(cp.mode === 'replay' && !cp.replay,'Use a fresh setup --mode replay; replay is single-use and never resumes an ambiguous segment');
  requireValue(flags.fixture,'--fixture prepared private replay JSON required');
  const fixturePath = path.resolve(flags.fixture); outsideRepository(fixturePath,repositoryRoot);
  requireValue(fs.statSync(fixturePath).size <= 2*REPLAY_LIMITS.sourceBytes,'Prepared fixture exceeds read budget');
  let replay;
  try { replay = validateReplay(readJson(fixturePath)); } catch { throw Error('Invalid prepared replay; content withheld'); }
  const segments = Number(flags.segments??3), mode = flags.control??'stream', profile = flags.profile??(rt.identity.capabilities.purpose === 'functional'?'none':'both'), probe = flags.probe??'composer';
  requireValue(Number.isInteger(segments) && segments >= 1 && segments <= REPLAY_LIMITS.segments,'segments must be 1..8');
  requireValue(['stream','quiet','other','mux'].includes(mode) && ['none','cpu','trace','both'].includes(profile) && ['composer','mesh'].includes(probe),'Expected control stream|quiet|other|mux, probe composer|mesh and profile none|cpu|trace|both');
  if (profile !== 'none') requireMeasurement(rt.identity,profile === 'cpu'?'cpu':'trace');
  requireValue(segments*replay.ends.length <= REPLAY_LIMITS.events && segments*(replay.wireBytes+Buffer.byteLength(JSON.stringify(replay.rows))) <= REPLAY_LIMITS.wireBytes,'Total replay event/byte budget exceeded');
  const [f,otherFixture] = cp.fixtures;
  if (probe === 'composer') { await visit(rt,cp,otherFixture); await visit(rt,cp,f); }
  const bindings = muxBindings(await rt.api('/api/mux/state')), live = bindings[f.tab], otherLive = bindings[otherFixture.tab];
  requireValue(live.agent_session_id === f.session && typeof live.session_binding_nonce === 'string' && live.session_binding_nonce.length > 0,'Exact owned session nonce required');
  requireValue(otherLive.agent_session_id === otherFixture.session && typeof otherLive.session_binding_nonce === 'string','Owned unrelated fixture nonce required');
  const other = {tab:otherFixture.tab,session:otherFixture.session,nonce:otherLive.session_binding_nonce};
  const authority = {tab:f.tab,session:f.session,nonce:live.session_binding_nonce,run:cp.run,other,muxTab:cp.terminal.tab,probe};
  const rows = ownedRows(replay,cp.run,f.session), tail = rows.at(-1);
  const otherRows = ownedRows(replay,cp.run+'-other',other.session), otherTail = otherRows.at(-1);
  const seededRows = [...rows.slice(0,-1),prefixRow(tail,replay.ends[0])], seededOtherRows = [...otherRows.slice(0,-1),prefixRow(otherTail,replay.ends[0])];
  const wireSize = value => Buffer.byteLength(JSON.stringify(value));
  const updateBytes = ['stream','other'].includes(mode)?replay.ends.reduce((sum,end) => sum+wireSize([prefixRow(mode === 'other'?otherTail:tail,end)]),0):0;
  const plannedWireBytes = segments*(wireSize(seededRows)+wireSize(seededOtherRows)+updateBytes+wireSize(otherRows)+(mode !== 'stream'?wireSize(rows):0));
  requireValue(plannedWireBytes <= REPLAY_LIMITS.wireBytes,'Owned replay wire byte budget exceeded');
  cp.replay = {fingerprint:replay.fingerprint,mode,segments,status:'running',authority};
  cp.pending = {replay:replay.fingerprint}; save();
  const report = {version:1,complete:false,identity:rt.identity,surface:rt.identity.surface??'native',authority,
    workload:{fingerprint:replay.fingerprint,historyRows:replay.historyRows,omittedOlder:replay.omittedOlder,segments,mode,profile,probe,
      chunkBytes:replay.chunkBytes,eventsPerSegment:replay.ends.length,wireBytesPerSegment:replay.wireBytes,plannedWireBytes,
      edit:probe === 'composer'?'trusted CDP Input.insertText x once per acknowledged chunk':'trusted CDP alternating mouseWheel deltaY +/-20 once per acknowledged chunk',cpuSamplingIntervalMicroseconds:2000},
    reconstruction:replay.reconstruction,exportHash:replay.exportHash,segments:[],limitations:[
      'Normalized test-event dispatch bypasses provider parsing, Rust ItemUpdated production, SSE decoding and ordinary listener authority adoption. Session/nonce and final store contents are independently verified.',
      'JSON parse and synchronous dispatch-to-store-return are directly timed; dispatch includes reducer/listener work and is not isolated adoption timing.',
      'CPU stages are statistical self samples classified by source URL/function, including instrumented store-verification calls. Trace layout/paint durations overlap. React commit has no direct Profiler timing here; unsupported stages stay unknown.',
      'Input timestamp-to-capture is a DOM timestamp proxy, not OS keyboard queue delay. CDP insertText does not reproduce physical keyboard/IME scheduling.',
      'EventTiming entries are supported eligible events at >=16ms only; absence is unknown, never zero queue delay. rAF and recordPaint are opportunities, not actual compositor presentation.',
      'Renderer long tasks include other mounted consumers. Profilers, CDP and post-segment GC perturb execution; compare identical options.',
      'Entry hash covers served HTML and entry scripts, not every HMR dependency. Parent must pin checkout/build and prohibit HMR during a run.',
      'Other control uses a separate owned transcript identity; mux uses the existing frontend SET_STATE test hook on an owned terminal status. Neither captures actual other-agent/SSE timing. Mux hook advances frontend generation only; use disposable DEV.',
      'Mesh probe counts graph DOM mutations and samples wheel handler/frame opportunities. DOM mutations are not React render counts; CPU/trace support graph/layout attribution without patching graph algorithms.',
      'Windows process CPU seconds include boundary sampling, GC and post-control final-tail restoration. Other platforms report this OS counter unknown. Renderer Performance metrics and CPU/trace cover the measured segment separately.',
    ]};
  let installed = false;
  try {
    report.boundary = await evaluate(rt.cdp,`(${installReplayPage.toString()})(${JSON.stringify(authority)})\n//# sourceURL=streaming-soak/replay-page.mjs`); installed = true;
    atomic(path.join(out,'replay-profile.json'),report);
    await rt.cdp.send('Performance.enable');
    for (let segment=1;segment<=segments;segment++) {
      await call('seed',JSON.stringify(seededRows));
      await call('seed',JSON.stringify(seededOtherRows),true);
      const baseline = await sample(rt,cp,out,`replay-${segment}-before`); save();
      const beforeMetrics = await rt.cdp.send('Performance.getMetrics');
      await call('begin');
      const measurements = await profiledSegment(segment);
      const page = await call('end');
      const afterMetrics = await rt.cdp.send('Performance.getMetrics');
      // Controls hold the local prefix during measurement; retain the complete saved tail afterward.
      if (mode !== 'stream') await call('seed',JSON.stringify(rows));
      await call('seed',JSON.stringify(otherRows),true);
      const completeness = assertComplete(rows,await call('read'),authority);
      completeness.other = assertComplete(otherRows,await call('readOther'),other);
      const after = await sample(rt,cp,out,`replay-${segment}-after`); save();
      const values = Object.fromEntries(Object.entries(page.values).map(([key,value]) => [key,distribution(value)]));
      requireValue(values.inputToSecondFrame.count === replay.ends.length && values.captureToDocumentBubble.count === replay.ends.length,'Every owned edit must have handler and frame evidence');
      report.segments.push({number:segment,values,longTaskPeaks:page.longTasks,timeline:page.timeline,graphMutations:page.graphMutations,eventTimingSupported:page.hasEventTiming,longTaskSupported:page.hasLongTask,
        draftCharacters:page.draftCharacters,completeness,baseline,after,pageMetrics:metricDelta(beforeMetrics,afterMetrics),...measurements});
      atomic(path.join(out,'replay-profile.json'),report);
    }
    const final = muxBindings(await rt.api('/api/mux/state'))[f.tab];
    requireValue(final.agent_session_id === authority.session && final.session_binding_nonce === authority.nonce && final.pty_id === f.pty,'Session/nonce/PTY changed during replay');
    report.complete = true; cp.replay.status = 'complete'; cp.pending = null;
    atomic(path.join(out,'replay-profile.json'),report); save();
  } finally {
    if (installed) await call('dispose').catch(() => {});
    await rt.cdp.send('Performance.disable').catch(() => {});
  }
  return report;

  function call(method,...args) { return evaluate(rt.cdp,`window.__streamingSoakReplay.${method}(${args.map(arg => JSON.stringify(arg)).join(',')})`); }
  async function profiledSegment(segment) {
    const tracing = profile === 'trace' || profile === 'both';
    const trace = tracing?traceCollector(200_000,{pid:rt.identity.renderer.pid}):null, cpu = profile === 'cpu' || profile === 'both';
    let cpuStarted = false, traceStarted = false, off = () => {}, removeComplete = () => {}, traceDone, cpuResult = null;
    try {
      if (cpu) {
        await rt.cdp.send('Profiler.enable'); await rt.cdp.send('Profiler.setSamplingInterval',{interval:2000});
        await rt.cdp.send('Profiler.start'); cpuStarted = true;
      }
      if (tracing) {
        off = rt.cdp.on('Tracing.dataCollected',({value}) => trace.ingest(value));
        traceDone = new Promise((resolve,reject) => {
          const signal = AbortSignal.timeout(450000);
          const fail = () => { remove(); reject(Error('Trace completion deadline')); };
          const remove = rt.cdp.on('Tracing.tracingComplete',() => { signal.removeEventListener('abort',fail); remove(); resolve(); });
          removeComplete = () => { signal.removeEventListener('abort',fail); remove(); };
          signal.addEventListener('abort',fail,{once:true});
        });
        traceDone.catch(() => {});
        await rt.cdp.send('Tracing.start',{categories:'devtools.timeline,disabled-by-default-devtools.timeline,blink.user_timing',transferMode:'ReportEvents'}); traceStarted = true;
      }
      for (const [index,end] of replay.ends.entries()) {
        const row = prefixRow(mode === 'other'?otherTail:tail,end), where = {segment,step:index+1,
          messageBytes:Buffer.byteLength(['quiet','mux'].includes(mode)?tail.content.slice(0,replay.ends[0]):row.content),historyRows:replay.historyRows};
        const point = await call('arm',where);
        const wire = ['stream','other'].includes(mode)?JSON.stringify([row]):null;
        await Promise.all([call('step',wire,mode),probe === 'composer'?rt.cdp.send('Input.insertText',{text:'x'}):
          rt.cdp.send('Input.dispatchMouseEvent',{type:'mouseWheel',...point,deltaX:0,deltaY:index%2?20:-20})]);
        await call('acknowledge');
        requireValue(!trace?.isOverflow(),'Trace event budget exceeded');
      }
    } finally {
      const cleanup = await Promise.allSettled([
        cpuStarted?rt.cdp.send('Profiler.stop').then(({profile:result}) => { cpuResult = summarizeCpu(result); }):Promise.resolve(),
        traceStarted?rt.cdp.send('Tracing.end').then(() => traceDone):Promise.resolve(),
      ]);
      off(); removeComplete();
      if (cpu) await rt.cdp.send('Profiler.disable');
      for (const result of cleanup) if (result.status === 'rejected') throw result.reason;
    }
    requireValue(!trace?.result().overflow,'Trace event budget exceeded');
    return {cpu:cpuResult,trace:tracing?trace.result():null,processAttribution:rt.identity.capabilities};
  }
}
function metricDelta(before,after) {
  const prior = Object.fromEntries(before.metrics.map(m => [m.name,m.value]));
  const names = ['TaskDuration','ScriptDuration','LayoutDuration','RecalcStyleDuration','LayoutCount','RecalcStyleCount'];
  return Object.fromEntries(names.map(name => [name,after.metrics.find(m => m.name === name)?.value-prior[name]]).map(([name,value]) => [name,Number.isFinite(value)?value:null]));
}
