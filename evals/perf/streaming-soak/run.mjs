import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {randomUUID,createHash} from 'node:crypto';
import {fixtureRecord} from '../chat-memory/fixture.mjs';
import {analyzeGraph,snapshotGraph,heapDiff} from './heap.mjs';
import {declareControl,plateau} from './oracle.mjs';
import {VERSION,requireValue,segmentSeconds,validateResume,cleanupPlan,sampleDue,acknowledge,validateFileOwnership,muxBindings} from './model.mjs';
import {atomic,readJson,waitFile,request,shellCommand} from './io.mjs';
import {attach,dcJson,visit,sample} from './runtime.mjs';

const [command,...args] = process.argv.slice(2);
const flags = Object.fromEntries(args.reduce((all,key,i) => key.startsWith('--')?[...all,[key.slice(2),args[i+1]]]:all,[]));
requireValue(['inspect','setup','pulse','sample','finish','cleanup','analyze','diff'].includes(command),'Expected inspect|setup|pulse|sample|finish|cleanup|analyze|diff');
const out = path.resolve(flags.out??''); requireValue(Boolean(flags.out),'--out required');
const checkpoint = path.join(out,'checkpoint.json');
const readSamples = () => fs.existsSync(path.join(out,'samples.jsonl')) ? fs.readFileSync(path.join(out,'samples.jsonl'),'utf8').trim().split('\n').filter(Boolean).map(JSON.parse) : [];
const save = () => atomic(checkpoint,cp);
let cp,rt;
const signal = AbortSignal.timeout(480_000);
const digest = file => createHash('sha256').update(fs.readFileSync(file)).digest('hex');

async function execute() {
  if (command === 'diff') {
    requireValue(flags.before && flags.after,'--before and --after heapsnapshots required');
    atomic(path.join(out,'heap-diff.json'),heapDiff(analyzeGraph(snapshotGraph(readJson(flags.before))),analyzeGraph(snapshotGraph(readJson(flags.after))))); return;
  }
  if (command === 'analyze') {
    cp = readJson(checkpoint); atomic(path.join(out,'plateau.json'),plateau(readSamples(),cp.control)); return;
  }
  rt = await attach(signal);
  if (command === 'inspect') { atomic(path.join(out,'identity.json'),rt.identity); return; }
  if (command === 'setup') return setup();
  cp = readJson(checkpoint);
  const state = await rt.api('/api/mux/state'), bindings = muxBindings(state);
  requireValue(state.workspaces.some(w => w.id === cp.workspace.id && w.cwd === cp.workspace.cwd),'Workspace identity drift');
  if (command === 'cleanup') cleanupPlan(cp,rt.identity,bindings);
  else validateResume(cp,rt.identity,bindings);
  verifyFiles();
  const config = readJson(path.join(out,'emitter.json'));
  requireValue(config.run === cp.run && config.token === cp.token && config.endpoint === cp.endpoint,'Emitter ownership drift');
  const message = (op,extra={}) => ({version:VERSION,run:cp.run,token:cp.token,op,...extra});
  if (cp.stopping && fs.existsSync(config.exit)) {
    cp.exit = readJson(config.exit);
    requireValue(cp.exit.run === cp.run && cp.exit.pid === cp.emitterPid && cp.exit.code === 0 && cp.exit.observed === 'child exit event','Interrupted stop exit mismatch');
    cp.stopping = false; save();
  }
  if (!cp.exit) {
    const status = await request(cp.endpoint,message('status'),AbortSignal.any([signal,AbortSignal.timeout(20_000)]));
    acknowledge(cp.batch,status);
  }
  if (command === 'pulse') {
    requireValue(!cp.exit,'Emitter has already exited');
    const seconds = segmentSeconds(Number(flags.seconds??420));
    requireValue(seconds <= 450,'Pulse reserves 30 seconds of the eight-minute cap for final evidence');
    const maxBatches = Number(flags['max-batches']??100_000);
    requireValue(Number.isSafeInteger(maxBatches) && maxBatches > 0 && maxBatches <= 100_000,'Invalid batch cap');
    const start = Date.now(); let batches = 0;
    const fixtureHasher = createHash('sha256').update(fs.readFileSync(cp.fixtures[0].path));
    cp.segment = {number:(cp.segment?.number??0)+1,startedAt:start,seconds,status:'running'}; save();
    while (Date.now()-start < seconds*1000 && batches < maxBatches) {
      if (sampleDue(Date.now(),cp.lastSample)) { await sample(rt,cp,out,'minute-'+Date.now()); save(); }
      if (cp.mode === 'load') {
        const f = cp.fixtures[0], row = fixtureRecord({session:f.session,cwd:cp.workspace.cwd,index:cp.batch+1,parent:f.parent,run:cp.run});
        cp.pending = {batch:cp.batch+1}; save();
        fs.appendFileSync(f.path,row.line); fixtureHasher.update(row.line); f.marker = row.marker; f.parent = row.uuid;
        const ack = await request(cp.endpoint,message('batch',{batch:cp.batch+1,lines:cp.lines}),AbortSignal.any([signal,AbortSignal.timeout(20_000)]));
        acknowledge(cp.batch+1,ack);
        await visit(rt,cp,f);
        await visit(rt,cp,cp.terminal);
        await terminalEvidence(cp.batch+1);
        cp.batch++; cp.pending = null; f.hash = fixtureHasher.copy().digest('hex'); save();
      } else {
        await visit(rt,cp,cp.fixtures[0]); await visit(rt,cp,cp.terminal);
      }
      batches++;
    }
    cp.segment.status = 'complete'; cp.segment.endedAt = Date.now(); cp.segment.batches = batches;
    await sample(rt,cp,out,'segment-'+cp.segment.number); save(); return;
  }
  if (command === 'sample') { await sample(rt,cp,out,'manual-'+Date.now(),{snapshot:flags.snapshot === 'yes'}); save(); return; }
  if (command === 'finish' || command === 'cleanup') {
    if (!cp.exit) {
      cp.stopping = true; save();
      const ack = await request(cp.endpoint,message('stop'),AbortSignal.any([signal,AbortSignal.timeout(20_000)])); acknowledge(cp.batch,ack);
      cp.exit = await waitFile(config.exit,AbortSignal.any([signal,AbortSignal.timeout(20_000)]));
      requireValue(cp.exit.run === cp.run && cp.exit.code === 0 && cp.exit.pid === cp.emitterPid && cp.exit.observed === 'child exit event','Emitter exit not proven');
      cp.stopping = false; save();
    }
    if (command === 'finish') {
      await sample(rt,cp,out,'finish',{snapshot:flags.snapshot !== 'no'}); save();
      const samples = readSamples();
      if (cp.mode === 'idle') atomic(path.join(out,'idle-control.json'),{version:1,mode:'idle',run:cp.run,completedAt:Date.now(),match:cp.match,samples});
      else atomic(path.join(out,'plateau.json'),plateau(samples,cp.control));
      return;
    }
    const tabs = cleanupPlan(cp,rt.identity,muxBindings(await rt.api('/api/mux/state')));
    for (const tab of tabs) { await dcJson(['tab','close','--tab',tab]); cp.cleanedTabs??=[]; cp.cleanedTabs.push(tab); save(); }
    fs.unlinkSync(cp.junction);
    for (const f of cp.fixtures) fs.unlinkSync(f.path);
    fs.rmdirSync(cp.history);
    await dcJson(['workspace','close',cp.workspace.id]);
    cp.stage = 'cleaned'; cp.cleanedAt = Date.now(); save();
    const state = await rt.api('/api/mux/state');
    requireValue(tabs.every(tab => !state.tabs[tab]) && !state.workspaces.some(w => w.id === cp.workspace.id),'Owned cleanup not observed');
  }
}
function verifyFiles() {
  const expectedHistory = path.join(cp.workspace.cwd,'streaming-soak-'+cp.run);
  const expectedJunction = path.join(os.homedir(),'.claude','projects',cp.workspace.cwd.replace(/[^a-zA-Z0-9]/g,'-'));
  requireValue(cp.junction === expectedJunction,'Fixture link path drift');
  for (const f of cp.fixtures) requireValue(path.dirname(f.path) === cp.history && path.basename(f.path) === f.session+'.jsonl','Fixture file path drift');
  validateFileOwnership(cp,{owner:readJson(path.join(out,'owner.json')).run,history:expectedHistory,
    junctionMatches:fs.lstatSync(cp.junction).isSymbolicLink() && fs.realpathSync(cp.junction) === fs.realpathSync(cp.history),
    inventoryMatches:fs.readdirSync(cp.history).sort().join('|') === cp.fixtures.map(f => path.basename(f.path)).sort().join('|'),
    hashes:Object.fromEntries(cp.fixtures.map(f => [f.path,digest(f.path)]))});
}
async function setup() {
  requireValue(rt.identity.build === flags['expect-build'] && rt.identity.entryHash === flags['expect-entry'],'Expected DEV build/served entry mismatch');
  const mode = flags.mode??'load'; requireValue(['load','idle'].includes(mode),'Expected load or idle');
  const run = randomUUID(), token = randomUUID();
  cp = {version:VERSION,stage:'setup',run,token,identity:rt.identity,mode,batch:0,pending:null,lastSample:null,fixtures:[],lines:16};
  cp.match = {build:cp.identity.build,entryHash:cp.identity.entryHash,renderer:cp.identity.renderer,daemon:cp.identity.daemon,
    platform:process.platform,cadenceMs:60_000,fixtureCount:1,lines:cp.lines,workloadVersion:1};
  if (mode === 'load') { requireValue(flags.control,'--control matched idle-control.json required'); cp.control = declareControl(readJson(flags.control),cp.match,Date.now()); }
  atomic(path.join(out,'owner.json'),{run}); save();
  const made = await dcJson(['workspace','create','Soak'+run.slice(0,8),'--provision']); cp.workspace = made.workspace??made; save();
  requireValue(cp.workspace.id && cp.workspace.cwd,'Missing provisioned workspace identity');
  cp.history = path.join(cp.workspace.cwd,'streaming-soak-'+run); fs.mkdirSync(cp.history);
  cp.junction = path.join(os.homedir(),'.claude','projects',cp.workspace.cwd.replace(/[^a-zA-Z0-9]/g,'-'));
  requireValue(!fs.existsSync(cp.junction),'Existing provider path refused'); save();
  fs.symlinkSync(cp.history,cp.junction,process.platform === 'win32'?'junction':'dir');
  const session = randomUUID(), fixture = {run,session,title:'Soakchat',path:path.join(cp.history,session+'.jsonl'),parent:null};
  const row = fixtureRecord({session,cwd:cp.workspace.cwd,index:0,run});
  fs.writeFileSync(fixture.path,row.line,{flag:'wx'}); fixture.parent = row.uuid; fixture.marker = row.marker; fixture.hash = digest(fixture.path);
  const tab = await dcJson(['tab','new','--workspace',cp.workspace.id,'--cwd',cp.workspace.cwd,'--title',fixture.title,'--eager','--description','Owned synthetic soak transcript; no provider launched']);
  fixture.tab = tab.tab_id??tab.id; cp.fixtures.push(fixture); save();
  await dcJson(['tab','rebind',session,'--tab',fixture.tab]);
  cp.endpoint = process.platform === 'win32' ? `\\\\.\\pipe\\domios-soak-${run}` : path.join(os.tmpdir(),`domios-soak-${run}.sock`);
  const config = {version:VERSION,fixtureOnly:true,run,token,endpoint:cp.endpoint,ready:path.join(out,'emitter-ready.json'),exit:path.join(out,'emitter-exit.json')};
  atomic(path.join(out,'emitter.json'),config); save();
  const emitter = fileURLToPath(new URL('./emitter.mjs',import.meta.url));
  const terminal = await dcJson(['tab','new','--workspace',cp.workspace.id,'--cwd',cp.workspace.cwd,'--title','Soakterm','--eager',
    '--description','Owned fixture emitter; no provider input','--cmd',shellCommand([process.execPath,emitter,'supervise',path.join(out,'emitter.json')])]);
  cp.terminal = {run,session:null,tab:terminal.tab_id??terminal.id,title:'Soakterm'}; save();
  const ready = await waitFile(config.ready,AbortSignal.any([signal,AbortSignal.timeout(30_000)]));
  requireValue(ready.run === cp.run && ready.endpoint === cp.endpoint,'Foreign emitter readiness'); cp.emitterPid = ready.pid;
  const bindings = muxBindings(await rt.api('/api/mux/state'));
  for (const f of [...cp.fixtures,cp.terminal]) f.pty = bindings[f.tab]?.pty_id;
  cp.stage = 'ready'; validateResume(cp,rt.identity,bindings); save();
  await visit(rt,cp,fixture); await visit(rt,cp,cp.terminal);
  await sample(rt,cp,out,'setup',{snapshot:flags.snapshot !== 'no'}); save();
}
async function terminalEvidence(batch) {
  const {evaluate} = await import('../renderer-memory/cdp.mjs');
  const {FIND_TERMINALS} = await import('../renderer-memory/terminals.mjs');
  await evaluate(rt.cdp,`new Promise((resolve,reject)=>{
    ${FIND_TERMINALS}
    const term=terminalsIn(document.querySelector('[data-tab-id="${cp.terminal.tab}"]'))[0];
    if(!term)return reject(Error('Owned terminal API missing'));
    let subscription;
    const deadline=AbortSignal.timeout(20000);
    const fail=()=>{subscription?.dispose();reject(Error('Terminal render evidence deadline'))};
    const check=()=>{for(let i=Math.max(0,term.buffer.normal.length-256);i<term.buffer.normal.length;i++){
      if(term.buffer.normal.getLine(i)?.translateToString().includes(${JSON.stringify('SOAK '+cp.run.slice(0,8)+' '+batch+':'+(cp.lines-1))})){
        subscription?.dispose();deadline.removeEventListener('abort',fail);resolve(true);return;
      }
    }};
    subscription=term.onWriteParsed(check);deadline.addEventListener('abort',fail,{once:true});check();
  })`);
}

fs.mkdirSync(out,{recursive:true});
requireValue(command !== 'setup' || !fs.existsSync(checkpoint),'Refuse existing checkpoint; resume with pulse');
const lock = path.join(out,'runner.lock');
const fd = fs.openSync(lock,'wx'); fs.writeSync(fd,JSON.stringify({pid:process.pid,command})); fs.closeSync(fd);
try {
  await Promise.race([execute(),new Promise((_,reject) => signal.addEventListener('abort',() => reject(Error('Eight-minute segment failure deadline')),{once:true}))]);
  process.stdout.write(JSON.stringify({command,out,status:'complete',run:cp?.run,batch:cp?.batch})+'\n');
} catch (error) {
  atomic(path.join(out,'failure.json'),{command,error:String(error.stack??error),at:Date.now(),pending:cp?.pending??null});
  process.stderr.write(String(error.stack??error)+'\n'); process.exitCode = 1;
} finally { rt?.close(); fs.unlinkSync(lock); }
