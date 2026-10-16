import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import {execFileSync} from 'node:child_process';
import {randomUUID} from 'node:crypto';
import {dcJson,dc} from '../../core-journeys/lib/env.mjs';
import {connect,evaluate,takeHeapSnapshot} from '../renderer-memory/cdp.mjs';
import {fixtureRecord} from './fixture.mjs';

const flags=Object.fromEntries(process.argv.slice(2).reduce((all,x,i,args)=>x.startsWith('--')?[...all,[x.slice(2),args[i+1]]]:all,[]));
const count=Number(flags.tabs??15),cycles=Number(flags.cycles??4),perCycle=Number(flags.records??64),initial=Number(flags.initial??480);
for(const [name,value,max] of [['tabs',count,15],['cycles',cycles,8],['records',perCycle,128],['initial',initial,600]]) {
  if(!Number.isSafeInteger(value)||value<1||value>max)throw Error(`Invalid ${name}`);
}
if(process.env.CODETERM_API_PORT!=='7686'||!process.env.CODETERM_DEV_DATA_DIR)throw Error('Shared DEV wrapper required');
const expected=flags['expect-build'];if(!/^[a-f0-9]{40}$/.test(expected??''))throw Error('Exact build required');
const run=randomUUID().slice(0,8),out=path.resolve(flags.out??`target-check/evals/chat-memory-${run}`);
fs.mkdirSync(out,{recursive:true});
const headers={Authorization:`Bearer ${process.env.CODETERM_API_TOKEN}`,'Content-Type':'application/json'};
const api=async route=>{const r=await fetch('http://127.0.0.1:7686'+route,{headers,signal:AbortSignal.timeout(15000)});if(!r.ok)throw Error(`API ${route}: ${r.status}`);return r.json();};
const health=await api('/api/health');if(health.build_sha!==expected)throw Error('Build drift');
const before=await api('/api/mux/state');
const created=[],samples=[],visits=[];let workspace,junction,cdp;
const result={status:'incomplete',run,health,count,cycles,perCycle,initial,created,samples,visits};
const save=()=>fs.writeFileSync(path.join(out,'result.json'),JSON.stringify(result,null,2));
const bound=AbortSignal.timeout(420000);
const append=(fixture,n)=>{
  let chunk='';for(let i=0;i<n;i++){const row=fixtureRecord({session:fixture.session,cwd:workspace.cwd,index:fixture.next++,parent:fixture.parent,run});fixture.parent=row.uuid;fixture.marker=row.marker;chunk+=row.line;}
  fs.appendFileSync(fixture.path,chunk);
};
const waitFunction=async(code)=>evaluate(cdp,`new Promise((resolve,reject)=>{const test=()=>(${code});if(test())return resolve(true);const o=new MutationObserver(()=>{if(test()){o.disconnect();resolve(true)}});o.observe(document,{subtree:true,attributes:true,childList:true,characterData:true});AbortSignal.timeout(20000).addEventListener('abort',()=>{o.disconnect();reject(Error('UI observation deadline'))},{once:true})})`);
const button=name=>`[...document.querySelectorAll('button')].filter(b=>b.getAttribute('aria-label')===${JSON.stringify(name)}).at(-1)`;
async function selectWorkspace(name){
  await evaluate(cdp,`(()=>{if(!(${button('Select workspace '+name)})){const b=[...document.querySelectorAll('button')].find(b=>b.getAttribute('aria-label')?.startsWith('Show all workspaces'));b?.click()}return true})()`);
  await waitFunction(button('Select workspace '+name));
  await evaluate(cdp,`(()=>{(${button('Select workspace '+name)}).click();return true})()`);
}
async function visit(fixture,phase){
  const begin=performance.now(),selector=`[data-testid="tab-session-chat"][data-tab-id="${fixture.tab}"]`;
  await waitFunction(button('Open tab '+fixture.title));
  await evaluate(cdp,`(()=>{(${button('Open tab '+fixture.title)}).click();return true})()`);
  await waitFunction(`document.querySelector(${JSON.stringify(selector)})?.dataset.snapshotReady==='true'`);
  const firstVisibleMs=performance.now()-begin;
  await evaluate(cdp,`new Promise(resolve=>requestAnimationFrame(()=>{const root=document.querySelector(${JSON.stringify(selector)}),s=root?.querySelector('[aria-label="Conversation"]');if(s)s.scrollTop=s.scrollHeight;requestAnimationFrame(resolve)}))`);
  await waitFunction(`document.querySelector(${JSON.stringify(selector)})?.innerText.includes(${JSON.stringify(fixture.marker)})`);
  const seen=await evaluate(cdp,`(()=>{const root=document.querySelector(${JSON.stringify(selector)});const ids=[...root.querySelectorAll('[data-source-message-id]')].map(e=>e.getAttribute('data-source-message-id'));return {rows:ids.length,unique:new Set(ids).size,markerVisible:root.innerText.includes(${JSON.stringify(fixture.marker)})}})()`);
  if(seen.rows!==seen.unique)throw Error('Duplicate mounted source ids');
  visits.push({phase,tab:fixture.tab,firstVisibleMs,readyMs:performance.now()-begin,...seen});save();
}
function processSample(){
  if(process.platform!=='win32')return null;
  const script="$rows=Get-Process -Name codeterm,msedgewebview2 -ErrorAction SilentlyContinue | ForEach-Object {try{[pscustomobject]@{pid=$_.Id;name=$_.ProcessName;created=$_.StartTime.ToUniversalTime().ToString('o');privateBytes=$_.PrivateMemorySize64;workingSet=$_.WorkingSet64}}catch{}};$rows | ConvertTo-Json -Compress";
  return JSON.parse(execFileSync('powershell.exe',['-NoLogo','-NoProfile','-Command',script],{encoding:'utf8',timeout:15000}));
}
async function sample(phase,{gc=false,snapshot=false}={}){
  if(gc)await cdp.send('HeapProfiler.collectGarbage');
  const heap=await cdp.send('Runtime.getHeapUsage'),dom=await cdp.send('Memory.getDOMCounters');
  const census=await evaluate(cdp,`({elements:document.querySelectorAll('*').length,chatRows:document.querySelectorAll('[data-source-message-id]').length,chatRoots:document.querySelectorAll('[data-testid="tab-session-chat"]').length,xterms:document.querySelectorAll('.xterm').length,canvases:document.querySelectorAll('canvas').length,callbacks:window.__TAURI_INTERNALS__?.callbacks?.size??null})`);
  const row={phase,at:new Date().toISOString(),gc,heap,dom,census,processes:processSample()};
  samples.push(row);save();console.log(`CHAT_MEMORY_SAMPLE phase=${phase} heap=${heap.usedSize} nodes=${dom.nodes}`);
  if(snapshot)await takeHeapSnapshot(cdp,path.join(out,phase+'.heapsnapshot'));
}
try{
  console.log(`CHAT_MEMORY_PREPARE out=${out} tabs=${count} build=${expected}`);
  const targets=await(await fetch(`http://127.0.0.1:${health.instance.cdp_port}/json/list`,{signal:AbortSignal.timeout(5000)})).json();
  const pages=targets.filter(t=>t.type==='page'&&new URL(t.url).port===String(health.instance.vite_port));
  if(pages.length!==1)throw Error('Exactly one native DEV page required');
  result.target={id:pages[0].id,url:pages[0].url};cdp=await connect(pages[0].webSocketDebuggerUrl);
  bound.addEventListener('abort',()=>cdp.close(),{once:true});
  await cdp.send('Performance.enable');await cdp.send('HeapProfiler.enable');
  await sample('before');
  const made=await dcJson(['workspace','create','Memsoak'+run,'--provision']);workspace=made.workspace??made;
  if(!workspace.id||!workspace.cwd)throw Error('Workspace identity missing');result.workspace=workspace;save();
  const history=path.join(workspace.cwd,'synthetic-memory');fs.mkdirSync(history,{recursive:true});
  junction=path.join(os.homedir(),'.claude','projects',workspace.cwd.replace(/[^a-zA-Z0-9]/g,'-'));
  if(fs.existsSync(junction))throw Error('Refusing existing provider fixture path');
  fs.symlinkSync(path.resolve(history),junction,process.platform==='win32'?'junction':'dir');result.junction=junction;save();
  for(let i=0;i<count;i++){
    const fixture={session:randomUUID(),title:'Mem'+String(i+1).padStart(2,'0'),next:0,parent:null};fixture.path=path.join(history,fixture.session+'.jsonl');
    fs.writeFileSync(fixture.path,'',{flag:'wx'});append(fixture,initial);
    const tab=await dcJson(['tab','new','--workspace',workspace.id,'--cwd',workspace.cwd,'--title',fixture.title,'--eager','--description','Synthetic JSONL memory fixture; no agent execution']);
    fixture.tab=tab.tab_id??tab.id;if(!fixture.tab)throw Error('Tab identity missing');created.push(fixture);save();
    await dcJson(['tab','rebind',fixture.session,'--tab',fixture.tab]);
  }
  const ready=await api('/api/mux/state');
  for(const f of created){const tab=ready.tabs[f.tab];if(tab?.agent_session_id!==f.session)throw Error('Fixture binding mismatch');f.provider=tab.agent_type;}
  save();await selectWorkspace(workspace.name);
  for(const f of created)await visit(f,'warm');
  await sample('warm',{gc:true,snapshot:flags.snapshots==='yes'});
  console.log(`CHAT_MEMORY_ARMED tabs=${created.length} providerLaunched=false`);
  for(let cycle=1;cycle<=cycles;cycle++){
    if(bound.aborted)throw Error('Scenario deadline');
    for(let slot=0;slot<created.length;slot++){
      const burst=Math.floor(perCycle/created.length)+(slot<perCycle%created.length?1:0);
      if(burst)for(const fixture of created)append(fixture,burst);
      await visit(created[slot],'cycle'+cycle);
    }
    await sample('cycle'+cycle);
  }
  await sample('paused',{gc:true,snapshot:flags.snapshots==='yes'});
  result.status='measured';
}catch(error){result.error=String(error);process.exitCode=1;console.log('CHAT_MEMORY_FAILED '+String(error));}
finally{
  if(cdp){try{const shot=await cdp.send('Page.captureScreenshot',{format:'png'});fs.writeFileSync(path.join(out,'final.png'),Buffer.from(shot.data,'base64'));}catch(error){result.screenshotError=String(error);}}
  result.cleanup=[];
  for(const fixture of created){const closed=await dc(['tab','close','--tab',fixture.tab]);result.cleanup.push({tab:fixture.tab,exit:closed.code});save();}
  if(junction){try{if(fs.lstatSync(junction).isSymbolicLink())fs.unlinkSync(junction);}catch(error){result.junctionCleanupError=String(error);}}
  if(workspace){const closed=await dc(['workspace','close',workspace.id]);result.workspaceCloseExit=closed.code;}
  if(cdp){try{const old=before.workspaces.find(w=>w.id===before.active_workspace_id);if(old)await selectWorkspace(old.name);const oldTab=before.tabs[before.focused_tab_id];if(oldTab)await evaluate(cdp,`(()=>{(${button('Open tab '+oldTab.title)})?.click();return true})()`);await sample('cleanup',{gc:true});}catch(error){result.cleanupSampleError=String(error);}cdp.close();}
  if(result.cleanup.some(x=>x.exit!==0)||result.workspaceCloseExit!==0)process.exitCode=1;
  save();console.log(`CHAT_MEMORY_COMPLETE status=${result.status} out=${out}`);
}
