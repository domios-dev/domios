import { openSync, closeSync, readSync, fstatSync, statSync } from 'node:fs';
import path from 'node:path';
import { readJson } from './runtime.mjs';

export function completedTurn(text, session, generation, historyBytes, userRecord = () => '#### user\n') {
  const lines = text.split('\n');
  lines.pop();
  const records = lines.filter(Boolean).map(line => JSON.parse(line));
  const start = records.findLast(record => record.kind === 'adapter_start');
  if (!start || start.sessionId !== session || start.processGeneration !== generation) throw new Error('adapter binding mismatch');
  const outcomes = records.filter(record => record.kind === 'turn_complete');
  const latest = outcomes.at(-1);
  if (!latest || latest.sessionId !== session || latest.processGeneration !== generation || latest.historyBytes !== historyBytes) return null;
  const record = outcomes.findLast(record => record.sessionId === session && record.processGeneration === generation && !blankUserRecord(userRecord(record)));
  if (!record) return null;
  if (!Number.isSafeInteger(record.turnSequence) || record.turnSequence < 1) throw new Error('invalid turn sequence');
  return { key: `${session}:${generation}:${record.turnSequence}`, outcome: record.outcome, complete: record.complete === true && record.outcome === 'answered' };
}

export function observed(binding, root) {
  const config = readJson(path.join(root, 'binding.json'), null);
  if (!config || config.worker !== binding.worker || config.lane !== binding.lane || !/^[a-zA-Z0-9_-]+$/.test(config.session || '') || !config.generation) throw new Error('exact binding.json required');
  const history = path.join(binding.lane, '.aider/history', `${config.session}.md`);
  try { return completedTurn(sidecarTail(`${history}.domios-turns.jsonl`), config.session, config.generation, statSync(history).size, record => readUserRecord(history, record)); }
  catch (error) { if (error.code === 'ENOENT') return null; throw error; }
}

export function sidecarTail(file) {
  const fd = openSync(file, 'r');
  try {
    const size = fstatSync(fd).size;
    const head = Buffer.alloc(Math.min(size, 4096));
    readSync(fd, head, 0, head.length, 0);
    if (size <= 65536) {
      const all = Buffer.alloc(size);
      readSync(fd, all, 0, size, 0);
      return all.toString('utf8');
    }
    const tail = Buffer.alloc(65536);
    readSync(fd, tail, 0, tail.length, size - tail.length);
    const first = head.toString('utf8').split('\n')[0];
    const last = tail.toString('utf8');
    return first + '\n' + last.slice(last.indexOf('\n') + 1);
  } finally { closeSync(fd); }
}


export function blankUserRecord(text) {
  return /^####[\t ]*(?:<blank>)?\s*$/.test(text.trim());
}

export function readUserRecord(file, record) {
  const start = record.userRecordStart, end = record.responseRecordStart;
  if (!Number.isSafeInteger(start) || !Number.isSafeInteger(end) || start < 0 || end < start || end > record.historyBytes) throw new Error('invalid user record byte bounds');
  const fd = openSync(file, 'r');
  try {
    const buffer = Buffer.alloc(Math.min(end - start, 65536));
    const bytes = readSync(fd, buffer, 0, buffer.length, start);
    if (bytes !== buffer.length) throw new Error('user record truncated');
    return buffer.toString('utf8');
  } finally { closeSync(fd); }
}
