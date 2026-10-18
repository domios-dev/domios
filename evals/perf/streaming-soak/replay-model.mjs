import {createHash} from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import {requireValue} from './model.mjs';

export const REPLAY_LIMITS = Object.freeze({sourceBytes:8*1024*1024,wireBytes:256*1024*1024,events:2048,history:400,segments:8});
export const hash = value => createHash('sha256').update(typeof value === 'string'?value:JSON.stringify(value)).digest('hex');
export function outsideRepository(file,root) {
  const check = candidate => {
    const relative = path.relative(fs.realpathSync(root),candidate);
    requireValue(relative === '..' || relative.startsWith('..'+path.sep) || path.isAbsolute(relative),'Private replay artifacts must be outside the repository');
  };
  check(path.resolve(file));
  let ancestor = path.resolve(file);
  while (!fs.existsSync(ancestor)) {
    const parent = path.dirname(ancestor); requireValue(parent !== ancestor,'Private artifact filesystem root unavailable'); ancestor = parent;
  }
  check(fs.realpathSync(ancestor));
  for (let directory = fs.statSync(ancestor).isDirectory()?fs.realpathSync(ancestor):path.dirname(fs.realpathSync(ancestor));;) {
    requireValue(!fs.existsSync(path.join(directory,'.git')),'Private replay artifacts must be outside every git checkout');
    const parent = path.dirname(directory); if (parent === directory) break; directory = parent;
  }
}
export function parseExport(text) {
  requireValue(Buffer.byteLength(text) <= REPLAY_LIMITS.sourceBytes,'Export exceeds source byte budget');
  let value;
  try { value = JSON.parse(text.replace(/^\uFEFF/,'')); } catch { throw Error('Invalid transcript export JSON; content withheld'); }
  const snapshot = value?.data?.messages?value.data:value;
  requireValue(Array.isArray(snapshot?.messages) && snapshot.messages.length > 0,'Expected supported tab chat JSON export with messages');
  requireValue(snapshot.pending === false && snapshot.count === snapshot.messages.length,'Require complete non-pending --since 0 export; count must match messages');
  requireValue(snapshot.messages.every(row => typeof row?.id === 'string' && typeof row.type === 'string' && typeof row.content === 'string'),'Invalid normalized transcript rows');
  requireValue(new Set(snapshot.messages.map(row => row.id)).size === snapshot.messages.length,'Duplicate exported row identities');
  return snapshot;
}
export function chunkEnds(text,chunkBytes) {
  requireValue(Number.isSafeInteger(chunkBytes) && chunkBytes >= 4 && chunkBytes <= 65536,'chunk-bytes must be 4..65536');
  const ends = []; let offset = 0, bytes = 0;
  for (const character of text) {
    const size = Buffer.byteLength(character);
    if (bytes && bytes+size > chunkBytes) { ends.push(offset); bytes = 0; }
    offset += character.length; bytes += size;
  }
  if (bytes) ends.push(offset);
  requireValue(ends.length > 0 && ends.length <= REPLAY_LIMITS.events,'Reconstructed stream exceeds event budget');
  return ends;
}
export function prepareReplay(text,{historyRows,chunkBytes=1024}={}) {
  const exported = parseExport(text), all = exported.messages, tail = all.at(-1);
  requireValue(tail.type === 'assistant' && tail.content.length > 0,'Export must end with the saved assistant response to reconstruct');
  requireValue(!tail.segments?.length || tail.segments.every(segment => segment.type === 'text') && tail.segments.map(segment => segment.text).join('') === tail.content,'Tail envelope cannot be reconstructed losslessly as text chunks');
  const history = historyRows ?? all.length-1;
  requireValue(Number.isSafeInteger(history) && history >= 0 && history <= Math.min(REPLAY_LIMITS.history,all.length-1),'history-rows must fit export and 400-row retention budget');
  const rows = structuredClone(all.slice(all.length-1-history));
  const ids = new Map(rows.map((row,index) => [row.id,'row-'+index]));
  for (const row of rows) {
    row.id = ids.get(row.id);
    if (row.sourceMessageId != null) row.sourceMessageId = ids.get(row.sourceMessageId) ?? row.id;
    if (ids.has(row.toolCallSourceId)) row.toolCallSourceId = ids.get(row.toolCallSourceId);
    delete row.sourceSessionId; delete row.inheritedFromSessionId; delete row.correlationId;
  }
  const ends = chunkEnds(tail.content,chunkBytes);
  const wireBytes = ends.reduce((sum,end) => sum+Buffer.byteLength(JSON.stringify(prefixRow(rows.at(-1),end))),0);
  requireValue(wireBytes <= REPLAY_LIMITS.wireBytes,'Cumulative stream exceeds wire byte budget; increase chunk-bytes');
  const body = {version:1,reconstruction:'UTF-8 bounded cumulative prefixes; original chunk boundaries/timings unavailable',
    exportHash:hash(text),exportedRows:all.length,omittedOlder:all.length-rows.length,historyRows:history,chunkBytes,ends,wireBytes,rows};
  return {...body,fingerprint:hash(body)};
}
export function validateReplay(value) {
  const {fingerprint,...body} = value ?? {};
  requireValue(body.version === 1 && fingerprint === hash(body),'Prepared replay fingerprint mismatch');
  requireValue(Array.isArray(body.rows) && body.rows.length === body.historyRows+1 && body.historyRows <= REPLAY_LIMITS.history,'Invalid replay history budget');
  const tail = body.rows.at(-1);
  requireValue(JSON.stringify(chunkEnds(tail.content,body.chunkBytes)) === JSON.stringify(body.ends),'Reconstructed chunk plan drift');
  requireValue(body.wireBytes === body.ends.reduce((sum,end) => sum+Buffer.byteLength(JSON.stringify(prefixRow(tail,end))),0) && body.wireBytes <= REPLAY_LIMITS.wireBytes,'Replay wire budget drift');
  return value;
}
export function prefixRow(row,end) {
  const next = {...row,content:row.content.slice(0,end)};
  if (row.segments?.length) {
    let remaining = end;
    next.segments = row.segments.map(segment => {
      const text = segment.text.slice(0,Math.max(0,remaining)); remaining -= text.length;
      return {...segment,text};
    });
  }
  return next;
}
export function ownedRows(replay,run,session) {
  const prefix = 'soak-'+run+'-';
  return replay.rows.map(row => ({...row,id:prefix+row.id,sourceMessageId:prefix+(row.sourceMessageId??row.id),sourceSessionId:session,
    ...(row.toolCallSourceId?.startsWith('row-')?{toolCallSourceId:prefix+row.toolCallSourceId}:{})}));
}
export function assertComplete(expected,actual,authority) {
  requireValue(actual?.authority?.sourceSessionId === authority.session && actual.authority.sessionBindingNonce === authority.nonce,'Replay authority drift');
  const selected = actual.rows.filter(row => expected.some(want => want.id === row.id));
  requireValue(selected.length === expected.length && hash(selected) === hash(expected),'Replay history or final tail incomplete');
  return {rows:selected.length,tailHash:hash(selected.at(-1)),rowsHash:hash(selected),retainedRows:actual.rows.length,evictedOlder:actual.evictedOlder};
}
export function compareReplays(before,after) {
  requireValue(before.complete === true && after.complete === true,'Incomplete replay cannot be compared');
  requireValue(hash(before.workload) === hash(after.workload),'Before/after inputs, history, segment count or profiling controls differ');
  requireValue(before.surface === after.surface,'Compare the same surface before/after; native/Chrome are separate controls');
  return {workload:before.workload,before:{identity:before.identity,segments:before.segments},after:{identity:after.identity,segments:after.segments},
    verdict:'Measured distributions and trends only; parent assesses responsiveness. No automatic latency threshold.'};
}
