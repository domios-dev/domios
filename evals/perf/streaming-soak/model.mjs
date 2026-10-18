import {isDeepStrictEqual} from 'node:util';

export const VERSION = 1;
export const SAMPLE_MS = 60_000;
export function requireValue(ok, reason) { if (!ok) throw Error(reason); }
export function segmentSeconds(value) {
  requireValue(Number.isInteger(value) && value > 0 && value <= 480, 'Segment must be 1..480 seconds');
  return value;
}
export function admitDevSurface({health,provenance,instance='',port=7686,surface='native',cdp,target}) {
  requireValue(port === 7686 && health?.status === 'ok' && Number(health.instance?.api_port) === port,
    'Shared DEV server identity required; PROD refused');
  requireValue(/^[a-f0-9]{40}$/.test(health.build_sha??'') && provenance?.schema === 3 &&
    provenance.buildSha === health.build_sha, 'Exact selected DEV provenance/build required');
  const selected = provenance.instance;
  requireValue(selected?.name === instance && (health.instance.name??'') === instance &&
    Number(selected.apiPort) === port, 'Selected DEV instance/API drift');
  requireValue(/^com\.codeterm\.dev(?:\.|$)/.test(selected.identifier??''), 'PROD client provenance refused');
  requireValue(['native','chrome'].includes(surface), 'Expected native or chrome surface');
  const gui = health.instance;
  const legacy = Boolean(gui.identifier || gui.vite_port || gui.cdp_port);
  const vite = Number(selected.vitePort), native = Number(selected.cdpPort);
  if (legacy) {
    requireValue(gui.identifier === selected.identifier && Number(gui.vite_port) === vite &&
      Number(gui.cdp_port) === native && Number.isInteger(vite) && vite > 0 && vite <= 65535 && vite !== 7685,
    'Legacy client target differs from selected provenance');
  }
  if (surface === 'native' && !legacy) return {available:false,reason:'Native CDP locator unavailable for independent server'};
  const cdpPort = surface === 'chrome'?Number(cdp):native;
  requireValue(Number.isInteger(cdpPort) && cdpPort >= 1024 && cdpPort <= 65535 && cdpPort !== 7685 && cdpPort !== port,
    'Invalid explicit DEV diagnostic port');
  requireValue(surface !== 'chrome' || Boolean(target) && (!legacy || cdpPort !== native),
    'Browser requires a separate explicit CDP port and target ID');
  return {available:true,surface,cdpPort,target:surface === 'chrome'?target:null,
    origin:`http://127.0.0.1:${legacy?vite:port}`,vitePort:legacy?vite:null};
}
export function validateDevSurfaceTarget(admission,target) {
  requireValue(admission.available && target?.type === 'page' && Boolean(target.id), 'DEV page target unavailable');
  requireValue(!admission.target || target.id === admission.target, 'Explicit Browser target mismatch');
  requireValue(new URL(target.url).origin === admission.origin, 'Browser target must serve the admitted DEV origin');
  const socket = new URL(target.webSocketDebuggerUrl);
  requireValue(socket.protocol === 'ws:' && socket.hostname === '127.0.0.1' && Number(socket.port) === admission.cdpPort &&
    socket.pathname === `/devtools/page/${target.id}`, 'DEV page diagnostic socket mismatch');
  return target;
}
export function functionalTailEvidence(expected,observed) {
  const tail = expected.at(-1);
  requireValue(tail && observed?.internalStore === 'unavailable' && observed.authority === 'backend_binding_only' &&
    observed.projection?.paintedTail === tail.id+'#'+tail.content.length && observed.visibleSourceIds?.includes(tail.sourceMessageId??tail.id),
  'Owned public projection/DOM tail evidence missing');
  return {tailId:tail.id,sourceId:tail.sourceMessageId??tail.id,characters:tail.content.length,projectionTail:observed.projection.paintedTail,
    internalStore:{available:false,reason:'bundled_transcript_store_not_exposed'},
    durableStorage:{available:false,reason:'frontend_test_event_is_not_persistence'},
    authority:{available:false,reason:'backend_binding_only'}};
}
export function measurementCapabilities(purpose,rendererCount) {
  requireValue(['profile','functional'].includes(purpose),'Expected purpose functional or profile');
  if (purpose === 'functional') return {purpose,rendererAttribution:'unavailable',traceAttribution:'unavailable',reason:'functional_page_only'};
  requireValue(rendererCount === 1,'Exactly one renderer required for supported process profiling');
  return {purpose,rendererAttribution:'exact',traceAttribution:'exact',reason:null};
}
export function requireMeasurement(identity,measurement) {
  requireValue(['memory','cpu','trace'].includes(measurement),'Unknown measurement capability');
  requireValue(identity?.capabilities?.purpose === 'profile' && identity.capabilities.rendererAttribution === 'exact' &&
    Number.isSafeInteger(identity.renderer?.pid) && identity.renderer.pid > 0 && Boolean(identity.renderer.created),
  `${measurement} unavailable without exact renderer attribution`);
}
export function rendererMetrics(identity,observed) {
  if (identity.capabilities?.purpose === 'functional') {
    requireValue(observed === null,'Functional mode cannot attribute an OS renderer');
    return {renderer:null,rendererPrivateBytes:null,rendererAttribution:{available:false,reason:'functional_page_only'}};
  }
  requireMeasurement(identity,'memory');
  requireValue(observed?.pid === identity.renderer.pid && observed.created === identity.renderer.created,'Renderer process identity changed');
  return {renderer:observed,rendererPrivateBytes:observed.privateBytes,rendererAttribution:{available:true,reason:null}};
}
export function validateIdentity(identity) {
  requireValue(identity?.port === 7686 && identity.dev === true, 'Shared DEV only; PROD refused');
  requireValue(/^[a-f0-9]{40}$/.test(identity.build ?? ''), 'Exact DEV SHA required');
  requireValue(/^[a-f0-9]{64}$/.test(identity.entryHash ?? ''), 'Served entry digest required');
  const functional = identity.capabilities?.purpose === 'functional';
  requireValue(isDeepStrictEqual(identity.capabilities,measurementCapabilities(functional?'functional':'profile',1)), 'Invalid explicit measurement capability');
  if (functional) {
    requireValue(identity.renderer === null,'Functional renderer attribution must be unavailable');
    requireValue(typeof identity.page?.origin === 'string' && identity.page.origin.length > 0 &&
      typeof identity.page.path === 'string' && identity.page.path.startsWith('/'),'Functional page identity required');
  }
  for (const key of functional?['daemon']:['renderer', 'daemon']) {
    requireValue(Number.isSafeInteger(identity[key]?.pid) && identity[key].pid > 0 &&
      typeof identity[key].created === 'string' && identity[key].created.length > 0, `Missing ${key} PID/creation`);
  }
  requireValue(Boolean(identity.target && identity.boot), 'Missing page/boot identity');
  return identity;
}
export function muxBindings(state) {
  const membership = new Map();
  for (const workspace of state.workspaces) for (const tab of workspace.tabs) {
    requireValue(!membership.has(tab),'Duplicate workspace membership'); membership.set(tab,workspace.id);
  }
  return Object.fromEntries(Object.entries(state.tabs).map(([id,tab]) => [id,{...tab,workspace_id:membership.get(id)??null}]));
}
export function validateResume(cp, actual, bindings) {
  requireValue(cp?.version === VERSION && cp.stage === 'ready', 'Checkpoint is not resumable');
  validateIdentity(cp.identity); validateIdentity(actual);
  requireValue(isDeepStrictEqual(cp.identity, actual), 'DEV identity drift');
  requireValue(Number.isSafeInteger(cp.batch) && cp.batch >= 0 && cp.pending === null, 'Ambiguous unfinished batch; never replay');
  requireValue(typeof cp.run === 'string' && /^[a-f0-9-]{36}$/.test(cp.run), 'Invalid owner');
  requireValue(Array.isArray(cp.fixtures) && cp.fixtures.length === (cp.mode === 'replay'?2:1) && cp.terminal?.tab, 'Missing owned fixtures');
  const owned = [...cp.fixtures, cp.terminal];
  requireValue(new Set(owned.map(f => f.tab)).size === owned.length, 'Duplicate owned tabs');
  for (const f of owned) {
    const live = bindings[f.tab];
    requireValue(typeof f.pty === 'string' && f.pty.length > 0 && typeof f.tab === 'string' && /^[a-zA-Z0-9_-]+$/.test(f.tab) &&
      f.run === cp.run && live?.workspace_id === cp.workspace.id &&
      (live.agent_session_id ?? null) === f.session && live.pty_id === f.pty &&
      (cp.mode !== 'replay' || (f.nonce??null) === (live.session_binding_nonce??null)), 'Owned tab/session/nonce/PTY drift');
  }
  return cp;
}
export function cleanupPlan(cp, actual, bindings) {
  validateResume({...cp, stage:'ready'}, actual, bindings);
  requireValue(cp.pending === null, 'Unfinished batch');
  const tabs = [...cp.fixtures.map(f => f.tab), cp.terminal.tab];
  requireValue(Object.entries(bindings).every(([id,tab]) => tab.workspace_id !== cp.workspace.id || tabs.includes(id)), 'Foreign tab in owned workspace');
  return tabs;
}
export function validateFileOwnership(cp,observed) {
  requireValue(observed.owner === cp.run && observed.history === cp.history && observed.junctionMatches && observed.inventoryMatches,'Owned fixture directory drift');
  requireValue(cp.fixtures.every(f => observed.hashes[f.path] === f.hash),'Fixture contents drift');
}
export function emitterDecision(state, message) {
  requireValue(message?.version === VERSION && message.run === state.run && message.token === state.token, 'Foreign emitter request');
  if (message.op === 'status') return {op:'status', batch:state.batch};
  requireValue(!state.stopped, 'Emitter stopped');
  if (message.op === 'stop') return {op:'stop', batch:state.batch};
  requireValue(message.op === 'batch' && Number.isSafeInteger(message.batch) && message.batch === state.batch + 1, 'Batch must be exact next number');
  requireValue(Number.isInteger(message.lines) && message.lines >= 1 && message.lines <= 128, 'Batch lines must be 1..128');
  return {op:'batch', batch:message.batch, lines:message.lines};
}
export function sampleDue(now, previous) { return previous === null || now - previous >= SAMPLE_MS; }
export function acknowledge(expected, reply) {
  requireValue(reply?.batch === expected && reply.ok === true, 'Exact batch acknowledgement missing');
}
