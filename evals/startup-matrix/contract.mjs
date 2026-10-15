import { readFileSync, existsSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { render, replyWindow } from './model.mjs';
import { chatMessages, replyVerdict } from '../core-journeys/lib/oracle.mjs';
export function validate(dir) {
  const r = JSON.parse(readFileSync(join(dir, 'results.json')));
  const errors = [];
  if (r.schema !== 1 || !Array.isArray(r.tabs) || !r.tabs.length) errors.push('Missing schema 1 tab matrix');
  for (const file of ['results.md', 'lock-holds.md', 'events.jsonl', 'before-state.json', 'boot-state.json', 'dev.log'])
    if (!existsSync(join(dir, file))) errors.push(`Missing ${file}`);
  if (readFileSync(join(dir, 'results.md'), 'utf8') !== render(r)) errors.push('results.md differs from JSON rendering');
  if (!r.restart?.verified) errors.push('Restart unproven');
  for (const t of r.tabs) {
    if (!['awake', 'asleep'].includes(t.policy) || typeof t.boot?.frozen !== 'boolean') errors.push(`${t.id}: classification absent`);
    if (t.ready_ms != null && (t.ready_event?.event_type !== 'tab-boot-stage' || t.ready_event?.data?.stage !== 'agent_ready' || t.ready_event.data.tab_id !== t.id))
      errors.push(`${t.id}: missing exact readiness event`);
    if (t.wake_ms != null && (t.wake_event?.data?.tab_id !== t.id || t.wake_event?.data?.stage !== 'agent_ready'))
      errors.push(`${t.id}: wake event absent`);
    if (t.reply_ok) {
      if (!t.reply_evidence || !existsSync(join(dir, t.reply_evidence))) errors.push(`${t.id}: transcript absent`);
      else {
        const proof = JSON.parse(readFileSync(join(dir, t.reply_evidence)));
        const window = replyWindow(chatMessages(proof.chat), proof.prompt);
        const verdict = replyVerdict({ messages: window?.following ?? [], baselineIds: new Set(proof.baseline_ids),
          marker: 'PONG', record: proof.record, boundSession: proof.session_before });
        if (!verdict.done) errors.push(`${t.id}: exact own-session PONG is unproven`);
      }
    }
    if ((!t.reply_ok || t.notes.length) && (!t.screenshot || !existsSync(join(dir, t.screenshot)))) errors.push(`${t.id}: failure screenshot absent`);
  }
  return { errors, failed: r.failures.length > 0 || r.tabs.some(t => !t.reply_ok || t.notes.length) };
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const result = validate(resolve(process.argv[2])); console.log(JSON.stringify(result, null, 2));
  process.exitCode = result.errors.length ? 1 : result.failed ? 3 : 0;
}
