import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import path from 'node:path';
import {claim} from './model.mjs';

export function runScopedIdentity(out,direction,provider,purpose) {
  const runRoot=path.resolve(path.dirname(out));
  const scope=createHash('sha256').update(runRoot).digest('hex').slice(0,16);
  const parts=[path.basename(out),direction,provider,purpose];
  for (const part of parts) assert.match(part,/^[a-zA-Z0-9_.-]+$/);
  return `B2_${scope}_${parts.join('_')}`;
}

export async function completeTurn({waitReply,snapshot,readActivity,tab,prompt,token,boundary}) {
  await waitReply();
  const after=await snapshot();
  const receipt=claim(after.proof,tab,prompt,boundary);
  assert.ok(receipt.assistant.map(row=>row.text).join('').includes(token),
    'Persisted same-turn assistant must answer supplied marker');
  const samples=await readActivity();
  const working={status:samples.some(sample=>sample.state==='working') ? 'PASS' : 'FAIL',samples};
  return {after,receipt,working};
}
