import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {dcJson} from '../../core-journeys/lib/env.mjs';
import {listenerIdentity} from '../../core-journeys/lib/listener.mjs';
import {connect,evaluate,pageTarget,takeHeapSnapshot} from '../renderer-memory/cdp.mjs';
import {FIND_TERMINALS} from '../renderer-memory/terminals.mjs';
import {processInfo} from './io.mjs';
import {requireValue,validateIdentity,admitDevSurface,validateDevSurfaceTarget,measurementCapabilities,requireMeasurement,rendererMetrics} from './model.mjs';
import {functionalReplayPrerequisites} from './replay-page.mjs';
import {ownedChatComposerMounted} from './readiness.mjs';
import {cleanupStateComplete,installCleanupObserverPage} from './cleanup-observation.mjs';

export {dcJson};
export async function attach(signal,options={}) {
  requireValue(process.env.CODETERM_API_PORT === '7686' && Boolean(process.env.CODETERM_DEV_DATA_DIR), 'Use run.sh dev_run; shared DEV only');
  const headers = {Authorization:`Bearer ${process.env.CODETERM_API_TOKEN}`};
  const api = async route => {
    const response = await fetch('http://127.0.0.1:7686'+route,{headers,signal});
    requireValue(response.ok,`DEV API ${route}: ${response.status}`); return response.json();
  };
  const health = await api('/api/health');
  const record = path.join(process.env.CODETERM_DEV_DATA_DIR,'dev-build-provenance.json');
  const metadata = fs.lstatSync(record);
  requireValue(metadata.isFile() && !metadata.isSymbolicLink() && metadata.size <= 4*1024*1024,'Invalid selected DEV provenance');
  const provenance = JSON.parse(fs.readFileSync(record,'utf8'));
  const admission = admitDevSurface({health,provenance,instance:process.env.CODETERM_INSTANCE??'',
    surface:options.surface??'native',cdp:options.cdp,target:options.target});
  requireValue(admission.available,admission.reason);
  const {surface,cdpPort,origin,vitePort} = admission;
  let target;
  if (surface === 'chrome') {
    const response = await fetch(`http://127.0.0.1:${cdpPort}/json/list`,{signal});
    requireValue(response.ok,'Browser diagnostic target list unavailable');
    const targets = (await response.json()).filter(t => t.id === options.target && t.type === 'page');
    requireValue(targets.length === 1,'Exactly one explicit Browser page target required');
    target = targets[0];
  } else target = await pageTarget(cdpPort,vitePort);
  validateDevSurfaceTarget(admission,target);
  const cdp = await connect(target.webSocketDebuggerUrl);
  signal.addEventListener('abort',() => cdp.close(),{once:true});
  let browser;
  try {
    const version = await (await fetch(`http://127.0.0.1:${cdpPort}/json/version`,{signal})).json();
    const purpose = options.purpose??'profile';
    let renderer = null, capabilities;
    if (purpose === 'functional') capabilities = measurementCapabilities(purpose);
    else {
      browser = await connect(version.webSocketDebuggerUrl);
      const {processInfo:processes} = await browser.send('SystemInfo.getProcessInfo');
      const renderers = processes.filter(p => p.type === 'renderer');
      capabilities = measurementCapabilities(purpose,renderers.length);
      renderer = processInfo(renderers[0].id);
    }
    const daemon = processInfo(listenerIdentity(7686).pid);
    if (purpose === 'functional') {
      const prerequisites = await evaluate(cdp,`(${functionalReplayPrerequisites.toString()})()`);
      requireValue(prerequisites.available,prerequisites.reason);
    }
    const native = await evaluate(cdp,'Boolean(window.__TAURI_INTERNALS__)');
    requireValue(native === (surface === 'native'),'Requested surface does not match native bridge presence');
    const htmlResponse = await fetch(target.url,{signal});
    requireValue(htmlResponse.ok && new URL(htmlResponse.url).origin === origin,'Entry HTML unavailable or redirected outside DEV');
    const htmlBytes = Buffer.from(await htmlResponse.arrayBuffer());
    const html = htmlBytes.toString('utf8');
    const entries = [...html.matchAll(/<script\b[^>]*\bsrc=["']([^"']+)["'][^>]*>/g)].map(m => m[1]);
    requireValue(entries.length > 0,'No served entry scripts');
    const hash = createHash('sha256').update(htmlBytes);
    for (const src of entries.sort()) {
      const url = new URL(src,target.url); requireValue(url.origin === origin,'Foreign entry origin');
      const response = await fetch(url,{signal}); requireValue(response.ok && new URL(response.url).origin === origin,'Entry script unavailable or redirected outside DEV');
      hash.update(src).update(Buffer.from(await response.arrayBuffer()));
    }
    const identity = validateIdentity({port:7686,dev:true,surface,capabilities,browser:{product:version.Browser??null,protocol:version['Protocol-Version']??null,userAgent:version['User-Agent']??null,v8:version['V8-Version']??null},
      page:{origin:new URL(target.url).origin,path:new URL(target.url).pathname,testMode:new URL(target.url).searchParams.has('e2e')},build:health.build_sha,entryHash:hash.digest('hex'),
      target:target.id,boot:health.boot_id,renderer:renderer?{pid:renderer.pid,created:renderer.created}:null,daemon:{pid:daemon.pid,created:daemon.created}});
    return {identity,cdp,api,signal,close:() => cdp.close()};
  } catch (error) { cdp.close(); throw error; }
  finally { browser?.close(); }
}

export async function observe(rt,expression,evidence='DOM evidence') {
  return evaluate(rt.cdp,`new Promise((resolve,reject)=>{
    const deadline=AbortSignal.timeout(20000);
    const fail=()=>{observer.disconnect();reject(Error(${JSON.stringify(`DOM evidence deadline: ${evidence}`)}))};
    const check=()=>{if((${expression})){observer.disconnect();deadline.removeEventListener('abort',fail);resolve(true)}};
    const observer=new MutationObserver(check);
    observer.observe(document,{subtree:true,childList:true,attributes:true,characterData:true});
    deadline.addEventListener('abort',fail,{once:true});check();
  })`);
}
export async function beginCleanupObservation(rt,tabs,workspace) {
  return evaluate(rt.cdp,`(${installCleanupObserverPage.toString()})(${JSON.stringify({tabs,workspace})},${cleanupStateComplete.toString()})`);
}
export async function waitCleanupObservation(rt) {
  return evaluate(rt.cdp,'window.__streamingSoakCleanup.wait()');
}
export async function disposeCleanupObservation(rt) {
  return evaluate(rt.cdp,'window.__streamingSoakCleanup?.dispose()');
}
export async function visit(rt,cp,fixture) {
  await dcJson(['workspace','focus',cp.workspace.id]);
  await dcJson(['tab','focus','--tab',fixture.tab]);
  if (fixture.session) {
    await observe(rt,`(${ownedChatComposerMounted.toString()})(document,${JSON.stringify(fixture.tab)})`,'owned chat composer mount');
    const root = `[data-testid="tab-session-chat"][data-tab-id="${fixture.tab}"]`;
    if (fixture.marker) await observe(rt,`document.querySelector(${JSON.stringify(root)})?.innerText.includes(${JSON.stringify(fixture.marker)})`);
  } else await observe(rt,`document.querySelector('[data-tab-id="${fixture.tab}"] .xterm') !== null`);
}
export async function sample(rt,cp,out,label,{snapshot=false}={}) {
  if (snapshot) requireMeasurement(rt.identity,'memory');
  const postGC = rt.identity.capabilities.purpose === 'profile';
  if (postGC) await rt.cdp.send('HeapProfiler.collectGarbage');
  const heap = await rt.cdp.send('Runtime.getHeapUsage'), dom = await rt.cdp.send('Memory.getDOMCounters');
  const census = await evaluate(rt.cdp,`(()=>{
    ${FIND_TERMINALS}
    const terminals=terminalsIn(document), registry=window.__internal_unstable_listeners_object_id__;
    return {mountedChats:document.querySelectorAll('[data-testid="tab-session-chat"]').length,
      xterms:document.querySelectorAll('.xterm').length,canvases:document.querySelectorAll('canvas').length,
      transcriptRows:document.querySelectorAll('[data-source-message-id]').length,
      transcriptWindows:document.querySelectorAll('[data-testid="tab-session-chat"] [aria-label="Conversation"]').length,
      outputRetention:terminals.length?terminals.map(t=>({lines:t.buffer.normal.length,cols:t.cols})):null,
      subscriptions:registry?Object.values(registry).reduce((n,v)=>n+Object.keys(v??{}).length,0):null,
      callbacks:window.__TAURI_INTERNALS__?.callbacks?.size??null,
      markdownCache:null,diagnosticRings:null,backendTranscriptRetention:null,backendOutputRetention:null};
  })()`);
  const daemon = processInfo(cp.identity.daemon.pid);
  requireValue(daemon.created === cp.identity.daemon.created,'Daemon process identity changed during sample');
  const observed = cp.identity.capabilities.purpose === 'functional'?null:processInfo(cp.identity.renderer.pid);
  const row = {at:Date.now(),label,postGC,jsBytes:heap.usedSize??null,
    ...rendererMetrics(cp.identity,observed),daemonPrivateBytes:daemon.privateBytes,
    daemon,dom,...census};
  fs.appendFileSync(path.join(out,'samples.jsonl'),JSON.stringify(row)+'\n');
  cp.lastSample = row.at;
  if (snapshot) await takeHeapSnapshot(rt.cdp,path.join(out,`${label}.heapsnapshot`));
  return row;
}
