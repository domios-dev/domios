import { watch } from 'node:fs';

const [api, ...paths] = process.argv.slice(2);
if (api !== 'http://127.0.0.1:17693' || process.env.CODETERM_INSTANCE !== 'blueprint-v2-proof') {
  throw new Error('wrong named DEV event target');
}
const token = process.env.CODETERM_API_TOKEN;
if (!token) throw new Error('missing authenticated DEV event capability');
const emit = (item) => process.stdout.write(`${JSON.stringify(item)}\n`);
const controller = new AbortController();
const watchers = paths.map((path) => watch(path, (event, filename) => {
  emit({ kind: 'file', path, event, filename: filename?.toString() ?? null });
}));
process.on('SIGTERM', () => {
  for (const watcher of watchers) watcher.close();
  controller.abort();
});
try {
  const response = await fetch(`${api}/api/events/stream`, {
    headers: { Authorization: `Bearer ${token}` }, signal: controller.signal,
  });
  if (!response.ok || !response.body) throw new Error(`event stream refused: ${response.status}`);
  emit({ kind: 'ready' });
  const decoder = new TextDecoder();
  let pending = '';
  for await (const chunk of response.body) {
    pending += decoder.decode(chunk, { stream: true }).replaceAll('\r\n', '\n');
    if (pending.length > 1024 * 1024) throw new Error('event frame exceeds bound');
    let end;
    while ((end = pending.indexOf('\n\n')) >= 0) {
      const frame = pending.slice(0, end);
      pending = pending.slice(end + 2);
      const data = frame.split('\n').filter((line) => line.startsWith('data:')).map((line) => line.slice(5).trimStart()).join('\n');
      if (!data) continue;
      const event = JSON.parse(data);
      if (event.event_type === 'overflow') throw new Error('event overflow; readiness cannot be proved');
      emit({ kind: 'event', event });
    }
  }
  throw new Error('event stream ended');
} catch (error) {
  if (!controller.signal.aborted) {
    emit({ kind: 'failure', reason: String(error) });
    process.exitCode = 1;
  }
} finally {
  for (const watcher of watchers) watcher.close();
}
