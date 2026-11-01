import fs from 'node:fs';
import path from 'node:path';
import {execFile} from 'node:child_process';

const outputLimit = 256 * 1024;
const roles = new Set(['button', 'textbox', 'combobox', 'heading', 'link', 'checkbox', 'switch']);
const isName = value => typeof value === 'string' && value.length > 0 && !value.startsWith('-');

function browserCall(args) {
  return new Promise((resolve, reject) => {
    execFile('domios', ['plugin', 'browser', ...args], {
      windowsHide: true, timeout: 130000, maxBuffer: outputLimit, encoding: 'utf8',
    }, (error, stdout, stderr) => {
      const result = {rawExit: error?.code ?? 0, stdout, stderr};
      if (error) reject(Object.assign(Error('Browser command failed; do not replay'), {result}));
      else resolve(result);
    });
  });
}

function save(dir, name, value) {
  fs.writeFileSync(path.join(dir, name), JSON.stringify(value, null, 2), {flag: 'wx', mode: 0o600});
}

function validateStep(step, origin) {
  const {verb, args} = step;
  if (!Array.isArray(args) || args.some(x => typeof x !== 'string') || JSON.stringify(args).length > 16384) {
    throw Error('bounded string argv required');
  }
  let valid = false;
  if (verb === 'goto' && args.length === 1) {
    const target = new URL(args[0]);
    valid = target.origin === origin && !target.username && !target.password;
  }
  if (verb === 'snapshot') valid = args.length === 0 || (args.length === 1 && /^(?:[1-9]\d{0,2}|1\d{3}|2000)$/.test(args[0]));
  if (verb === 'click' || verb === 'type') {
    valid = args.length === (verb === 'click' ? 2 : 3) && roles.has(args[0]) && isName(args[1]) &&
      (verb !== 'type' || !args[2].startsWith('-'));
    if (step.once !== true) throw Error('mutation requires explicit once declaration');
  }
  if (verb === 'wait') {
    valid = args.length === 8 && args[0] === '--role' && roles.has(args[1]) &&
      args[2] === '--name' && isName(args[3]) && args[4] === '--state' &&
      ['visible', 'hidden', 'enabled', 'expanded', 'checked', 'unchecked', 'selected', 'unselected'].includes(args[5]) &&
      args[6] === '--timeout' && /^(?:[1-9]|[1-9]\d|1[01]\d|120)s$/.test(args[7]);
  }
  if (!valid) throw Error('unsupported semantic step or foreign navigation');
  if (step.expect !== undefined && (typeof step.expect !== 'string' || !step.expect.length)) {
    throw Error('nonempty fixture state assertion required');
  }
}

export async function runSteps({origin, steps, out, call = browserCall}) {
  const target = new URL(origin);
  if (target.protocol !== 'http:' || target.hostname !== '127.0.0.1' || !target.port ||
      ['7685', '7686'].includes(target.port) || target.pathname !== '/' || target.search ||
      target.hash || target.username || target.password) throw Error('explicit isolated loopback origin required');
  if (!Array.isArray(steps) || !steps.length || steps.length > 30) throw Error('bounded steps required');
  const ids = new Set();
  for (const step of steps) {
    if (typeof step.id !== 'string' || !/^[a-z][a-z0-9-]{0,63}$/.test(step.id) || ids.has(step.id)) throw Error('invalid or duplicate step');
    ids.add(step.id);
    validateStep(step, target.origin);
  }
  steps = structuredClone(steps);
  fs.mkdirSync(out);
  for (const [index, step] of steps.entries()) {
    const stem = `${index + 1}-${step.id}`;
    save(out, `${stem}-attempt.json`, {id: step.id, verb: step.verb, at: new Date().toISOString(), once: step.once === true});
    try {
      const result = await call([step.verb, ...step.args]);
      if (result.rawExit !== 0) throw Object.assign(Error('nonzero Browser result'), {result});
      if (Buffer.byteLength(result.stdout ?? '') + Buffer.byteLength(result.stderr ?? '') > outputLimit) throw Error('output overflow');
      if (step.expect && !String(result.stdout).includes(step.expect)) {
        throw Object.assign(Error('expected observable state missing'), {result});
      }
      save(out, `${stem}-result.json`, {state: 'observed', ...result});
    } catch (error) {
      save(out, `${stem}-failure.json`, {
        state: ['click', 'type', 'goto'].includes(step.verb) ? 'action_unconfirmed' : 'failed',
        reason: error.message, result: error.result ?? null,
      });
      throw error;
    }
  }
  save(out, 'completed.json', {scope: 'selected scripted steps only; not whole-case PASS', steps: steps.length});
  return {steps: steps.length};
}

export function ownTurnVerdict({session, actualSession, nonce, reportedNonce, beforeIds, messages, payload, completedReplyIds = []}) {
  if (!session || session !== actualSession || !nonce || nonce !== reportedNonce) {
    return {state: 'refused', reason: 'own binding mismatch'};
  }
  if (typeof payload !== 'string' || !payload.length || !Array.isArray(beforeIds) ||
      !Array.isArray(messages) || !Array.isArray(completedReplyIds) ||
      messages.some(m => !m || typeof m.id !== 'string' || !m.id) ||
      new Set(messages.map(m => m.id)).size !== messages.length ||
      beforeIds.length > messages.length || beforeIds.some((id, i) => messages[i].id !== id)) {
    return {state: 'refused', reason: 'invalid append window'};
  }
  const fresh = messages.slice(beforeIds.length);
  const users = fresh.filter(m => m.type === 'user' && m.content === payload);
  if (users.length !== 1) {
    return {state: 'unconfirmed', reason: 'expected one exact own-session user turn', count: users.length};
  }
  const start = fresh.indexOf(users[0]);
  const end = fresh.findIndex((m, i) => i > start && m.type === 'user');
  const replies = fresh.slice(start + 1, end < 0 ? undefined : end)
    .filter(m => m.type === 'assistant' && typeof m.content === 'string' && m.content.trim());
  const complete = replies.length > 0 && completedReplyIds.includes(replies.at(-1).id);
  return {state: complete ? 'roundtrip_observed' : 'delivered', userId: users[0].id, replyIds: replies.map(m => m.id)};
}
