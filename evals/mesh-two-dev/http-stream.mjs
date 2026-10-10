import { validateTarget } from './sampling.mjs';
import { streamPaths, collectState } from './stream.mjs';
export function devStreamClient({ port, token, peer, fetcher = fetch }) {
  validateTarget('aiderqa', port);
  if (!token) throw new Error('Bench-resolved DEV credentials required');
  if (peer && !/^[a-f0-9-]{6,}$/.test(peer)) throw new Error('Pinned windows identity required');
  const base = `http://127.0.0.1:${port}`, paths = streamPaths(peer);
  async function request(path, signal, body) {
    const response = await fetcher(base + path, { method: body ? 'POST' : 'GET', headers: { Authorization: `Bearer ${token}`, ...(body ? { 'Content-Type': 'application/json' } : {}) }, body: body ? JSON.stringify(body) : undefined, signal });
    if (!response.ok) throw new Error(`DEV stream HTTP ${response.status}`);
    return response;
  }
  return {
    open: async signal => {
      const response = await request(paths.stream, signal);
      if (!response.body) throw new Error('Missing SSE stream');
      return response.body;
    },
    fetchState: async signal => {
      const readSignal = AbortSignal.any([signal, AbortSignal.timeout(10000)]);
      const response = await request(paths.state, readSignal, peer ? { target_peer_id: peer, method: 'GET', path: '/api/mux/state', body: null } : null);
      const state = await response.json();
      if (!peer) return state;
      if (state.ok !== true || !Number.isInteger(state.status) || state.status < 200 || state.status >= 300 || state.data == null) throw new Error('Target state relay refused');
      return state.data;
    },
  };
}
export async function captureStream(options, write, signal) {
  const client = devStreamClient(options);
  const chunks = await client.open(signal);
  return collectState({ chunks, fetchState: client.fetchState, write, signal });
}
