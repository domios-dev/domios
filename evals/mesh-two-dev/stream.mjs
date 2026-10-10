export class SseParser {
  buffer = '';
  push(text) {
    this.buffer += text;
    const events = [];
    let match;
    while ((match = /\r?\n\r?\n/.exec(this.buffer))) {
      const frame = this.buffer.slice(0, match.index);
      this.buffer = this.buffer.slice(match.index + match[0].length);
      const data = frame.split(/\r?\n/).filter(line => line.startsWith('data:')).map(line => line.slice(5).replace(/^ /, '')).join('\n');
      if (data) events.push(JSON.parse(data));
    }
    return events;
  }
  finish() { if (this.buffer.trim()) throw new Error('Truncated SSE frame'); }
}
export class EventOrder {
  cursor = null;
  accept(frame) {
    if (frame.event_type === 'overflow') {
      this.cursor = null;
      return { resyncRequired: true, reason: frame.data?.reason };
    }
    if (!frame.boot || !Number.isInteger(frame.seq) || frame.seq < 0) throw new Error('Invalid event cursor');
    if (this.cursor) {
      if (frame.boot !== this.cursor.boot) throw new Error('Boot changed without resync');
      if (frame.seq !== this.cursor.seq + 1) throw new Error(`Event sequence lost, duplicated or reordered: ${this.cursor.seq}->${frame.seq}`);
    }
    this.cursor = { boot: frame.boot, seq: frame.seq };
    return { resyncRequired: false };
  }
}
export async function collectState({ chunks, fetchState, write, signal, now = Date.now }) {
  const parser = new SseParser(), order = new EventOrder();
  const decoder = new TextDecoder();
  for await (const chunk of chunks) {
    signal.throwIfAborted();
    for (const frame of parser.push(typeof chunk === 'string' ? chunk : decoder.decode(chunk, { stream: true }))) {
      const receivedMs = now();
      await write({ kind: 'sse', frame, receivedMs });
      const transition = order.accept(frame);
      if (transition.resyncRequired || ['mux-state-changed', 'mesh-state-changed', 'tab-status-changed'].includes(frame.event_type)) {
        const state = await fetchState(signal);
        await write({ kind: 'state', frame, receivedMs, sampledMs: now(), state });
      }
    }
  }
  parser.push(decoder.decode());
  parser.finish();
  throw new Error('State stream ended; readiness not established');
}
export function streamPaths(peer) {
  if (!peer) return { stream: '/api/events/stream', state: '/api/mux/state' };
  const query = new URLSearchParams({ peer, path: '/api/events/stream' });
  return { stream: `/api/mesh/relay-stream?${query}`, state: '/api/mesh/relay-to', peer };
}
