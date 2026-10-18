import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {dcJson} from '../../core-journeys/lib/env.mjs';
import {listenerIdentity} from '../../core-journeys/lib/listener.mjs';
import {connect,evaluate,pageTarget,takeHeapSnapshot} from '../renderer-memory/cdp.mjs';
import {FIND_TERMINALS} from '../renderer-memory/terminals.mjs';
import {processInfo} from './io.mjs';
import {requireValue,validateIdentity} from './model.mjs';

export {dcJson};
export async function attach(signal,options={}) {
  requireValue(process.env.CODETERM_API_PORT === '7686' && Boolean(process.env.CODETERM_DEV_DATA_DIR), 'Use run.sh dev_run; shared DEV only');
  const headers = {Authorization:`Bearer ${process.env.CODETERM_API_TOKEN}`};
  const api = async route => {
    const response = await fetch('http://127.0.0.1:7686'+route,{headers,signal});
    requireValue(response.ok,`DEV API ${route}: ${response.status}`); return response.json();
  };
  const health = await api('/api/health');
  requireValue(Number(health.instance?.api_port) === 7686,'DEV listener mismatch');
  requireValue(/^com\.codeterm\.dev(?:\.|$)/.test(health.instance.identifier??''),'PROD build identifier refused');
  const nativePort = Number(health.instance.cdp_port), vitePort = Number(health.instance.vite_port);
  const surface = options.surface??'native';
  requireValue(['native','chrome'].includes(surface),'Expected native or chrome surface');
  const cdpPort = surface === 'chrome'?Number(options.cdp):nativePort;
  requireValue(surface !== 'chrome' || cdpPort !== nativePort && Boolean(options.target),'Chrome requires a separate explicit CDP port and target ID');
  requireValue(cdpPort > 0 && cdpPort !== 7685 && vitePort > 0 && vitePort !== 7685,'Invalid DEV frontend ports');
  let target;
  if (surface === 'chrome') {
    const targets = await (await fetch(`http://127.0.0.1:${cdpPort}/json/list`,{signal})).json();
    target = targets.find(t => t.id === options.target && t.type === 'page');
    requireValue(target && new URL(target.url).origin === `http://127.0.0.1:${vitePort}`,'Explicit Chrome target must serve this DEV entry');
  } else target = await pageTarget(cdpPort,vitePort);
  const cdp = await connect(target.webSocketDebuggerUrl);
  signal.addEventListener('abort',() => cdp.close(),{once:true});
  let browser;
  try {
    const version = await (await fetch(`http://127.0.0.1:${cdpPort}/json/version`,{signal})).json();
    browser = await connect(version.webSocketDebuggerUrl);
    const {processInfo:processes} = await browser.send('SystemInfo.getProcessInfo');
    const renderers = processes.filter(p => p.type === 'renderer');
    requireValue(renderers.length === 1,'Exactly one DEV renderer required; refuse ambiguous PID attribution');
    const renderer = processInfo(renderers[0].id), daemon = processInfo(listenerIdentity(7686).pid);
    const native = await evaluate(cdp,'Boolean(window.__TAURI_INTERNALS__)');
    requireValue(native === (surface === 'native'),'Requested surface does not match native bridge presence');
    const origin = `http://127.0.0.1:${vitePort}`;
    const htmlResponse = await fetch(target.url,{signal}); requireValue(htmlResponse.ok,'Entry HTML unavailable');
    const html = await htmlResponse.text();
    const entries = [...html.matchAll(/<script\b[^>]*\bsrc=["']([^"']+)["'][^>]*>/g)].map(m => m[1]);
    requireValue(entries.length > 0,'No served entry scripts');
    const hash = createHash('sha256').update(html);
    for (const src of entries.sort()) {
      const url = new URL(src,target.url); requireValue(url.origin === origin,'Foreign entry origin');
      const response = await fetch(url,{signal}); requireValue(response.ok,'Entry script unavailable');
      hash.update(src).update(await response.text());
    }
    const identity = validateIdentity({port:7686,dev:true,surface,browser:{product:version.Browser??null,protocol:version['Protocol-Version']??null,userAgent:version['User-Agent']??null,v8:version['V8-Version']??null},
      page:{origin:new URL(target.url).origin,path:new URL(target.url).pathname,testMode:new URL(target.url).searchParams.has('e2e')},build:health.build_sha,entryHash:hash.digest('hex'),
      target:target.id,boot:health.boot_id,renderer:{pid:renderer.pid,created:renderer.created},daemon:{pid:daemon.pid,created:daemon.created}});
    return {identity,cdp,api,signal,close:() => cdp.close()};
  } catch (error) { cdp.close(); throw error; }
  finally { browser?.close(); }
}

export async function observe(rt,expression) {
  return evaluate(rt.cdp,`new Promise((resolve,reject)=>{
    const deadline=AbortSignal.timeout(20000);
    const fail=()=>{observer.disconnect();reject(Error('DOM evidence deadline'))};
    const check=()=>{if((${expression})){observer.disconnect();deadline.removeEventListener('abort',fail);resolve(true)}};
    const observer=new MutationObserver(check);
    observer.observe(document,{subtree:true,childList:true,attributes:true,characterData:true});
    deadline.addEventListener('abort',fail,{once:true});check();
  })`);
}
export async function visit(rt,cp,fixture) {
  await dcJson(['workspace','focus',cp.workspace.id]);
  await dcJson(['tab','focus','--tab',fixture.tab]);
  if (fixture.session) {
    const root = `[data-testid="tab-session-chat"][data-tab-id="${fixture.tab}"]`;
    await observe(rt,`document.querySelector(${JSON.stringify(root)})?.dataset.snapshotReady === 'true'`);
    if (fixture.marker) await observe(rt,`document.querySelector(${JSON.stringify(root)})?.innerText.includes(${JSON.stringify(fixture.marker)})`);
  } else await observe(rt,`document.querySelector('[data-tab-id="${fixture.tab}"] .xterm') !== null`);
}
export async function sample(rt,cp,out,label,{snapshot=false}={}) {
  await rt.cdp.send('HeapProfiler.collectGarbage');
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
  const renderer = processInfo(cp.identity.renderer.pid), daemon = processInfo(cp.identity.daemon.pid);
  for (const [key,value] of [['renderer',renderer],['daemon',daemon]])
    requireValue(value.created === cp.identity[key].created,'Process identity changed during sample');
  const row = {at:Date.now(),label,postGC:true,jsBytes:heap.usedSize??null,
    rendererPrivateBytes:renderer.privateBytes,daemonPrivateBytes:daemon.privateBytes,
    renderer,daemon,dom,...census};
  fs.appendFileSync(path.join(out,'samples.jsonl'),JSON.stringify(row)+'\n');
  cp.lastSample = row.at;
  if (snapshot) await takeHeapSnapshot(rt.cdp,path.join(out,`${label}.heapsnapshot`));
  return row;
}
