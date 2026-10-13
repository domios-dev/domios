import { readFileSync, writeFileSync } from 'node:fs';

const [timeout, track, output, baseline, ...cleared] = process.argv.slice(2);
const tracked = readFileSync(track, 'utf8').trim().split('\n').filter(Boolean).map(row => row.split(' ')[0]);
if (tracked.length === 0) throw new Error('empty fleet');
const previous = baseline ? new Map(JSON.parse(readFileSync(baseline, 'utf8')).map(tab => [tab.id, tab.agent_session_id])) : new Map();
const api = process.env.CODETERM_API_URL;
const headers = { Authorization: `Bearer ${process.env.CODETERM_API_TOKEN}` };
const signal = AbortSignal.timeout(Number(timeout) * 1000);

async function bound() {
  const response = await fetch(`${api}/api/tabs`, { headers, signal });
  if (!response.ok) throw new Error(`tabs snapshot: ${response.status}`);
  const tabs = await response.json();
  writeFileSync(output, JSON.stringify(tabs));
  const byId = new Map(tabs.map(tab => [tab.id, tab]));
  return tracked.every(id => byId.get(id)?.agent_session_id)
    && cleared.every(id => previous.get(id) && byId.get(id)?.agent_session_id !== previous.get(id));
}

async function run() {
  const response = await fetch(`${api}/api/events/stream`, { headers, signal });
  if (!response.ok || !response.body) throw new Error(`event stream: ${response.status}`);
  const reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
  try {
    if (await bound()) return;
    let buffer = '';
    for (;;) {
      const { done, value } = await reader.read();
      if (done) throw new Error('stream ended before fleet binding');
      buffer += value.replaceAll('\r', '');
      let boundary;
      while ((boundary = buffer.indexOf('\n\n')) >= 0) {
        const frame = buffer.slice(0, boundary);
        buffer = buffer.slice(boundary + 2);
        if (frame.split('\n').some(line => line.startsWith('data:')) && await bound()) return;
      }
    }
  } finally {
    await reader.cancel();
  }
}
run().catch(error => {
  console.error(`FAIL fleet binding: ${error.message}`);
  process.exitCode = 1;
});
