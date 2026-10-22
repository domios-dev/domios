import { pathToFileURL } from 'node:url';

export function healthReady(health) {
  return health?.status === 'ok' && health.startup?.state === 'ready';
}

export async function waitForReady({ origin, token, signal, fetcher = fetch }) {
  const headers = { Accept: 'text/event-stream', ...(token ? { Authorization: `Bearer ${token}` } : {}) };
  for (;;) {
    signal.throwIfAborted();
    const response = await fetcher(new URL('/api/startup/events', origin), { headers, signal });
    if (!response.ok) throw new Error(`Startup events refused: HTTP ${response.status}`);
    if (response.headers.get('content-type')?.split(';')[0] !== 'text/event-stream' || !response.body) {
      throw new Error('Daemon does not support startup readiness events');
    }
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = '';
    try {
      for (;;) {
        const chunk = await reader.read();
        signal.throwIfAborted();
        if (chunk.done) break;
        buffer += decoder.decode(chunk.value, { stream: true });
        let boundary;
        while ((boundary = /\r?\n\r?\n/.exec(buffer))) {
          const frame = buffer.slice(0, boundary.index);
          buffer = buffer.slice(boundary.index + boundary[0].length);
          if (frame.length > 64 * 1024) throw new Error('Startup event exceeds capacity');
          const lines = frame.split(/\r?\n/);
          if (!lines.includes('event: startup-state')) continue;
          const snapshot = JSON.parse(lines.filter(line => line.startsWith('data:'))
            .map(line => line.slice(5).trimStart()).join('\n'));
          if (snapshot.state === 'failed') throw new Error('Server initialization failed');
          if (snapshot.state === 'ready') {
            const health = await fetcher(new URL('/api/health', origin), { headers, signal });
            if (!health.ok || !healthReady(await health.json())) throw new Error('Server health is not ready');
            return;
          }
          if (snapshot.state !== 'starting') throw new Error('Invalid startup state');
        }
        if (buffer.length > 64 * 1024) throw new Error('Startup event exceeds capacity');
      }
    } finally {
      await reader.cancel().catch(() => {});
      reader.releaseLock();
    }
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    if (process.argv[2] === 'health-ready') {
      let input = '';
      for await (const chunk of process.stdin) input += chunk;
      process.exitCode = healthReady(JSON.parse(input)) ? 0 : 1;
    } else if (process.argv[2] === 'wait') {
      const seconds = Number(process.argv[4] ?? '120');
      if (!Number.isFinite(seconds) || seconds <= 0) throw new Error('Positive startup deadline required');
      await waitForReady({ origin: process.argv[3], token: process.env.CODETERM_API_TOKEN,
        signal: AbortSignal.timeout(seconds * 1000) });
    } else throw new Error('Expected health-ready or wait');
  } catch (error) {
    console.error(`Server readiness failed: ${error.message}`);
    process.exitCode = 1;
  }
}
