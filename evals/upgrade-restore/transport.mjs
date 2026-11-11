// WHAT: In-process daemon reads for the replay verifier, composed once through the bench lib.
// WHY: One shell plus lib resolution per CLI call spent the whole 90 s deadline before any tab verdict.
const PROD_PORT = 7685;
const SHARED_DEV_PORT = 7686;
const ISOLATED_PREFIX = 'upgrade-restore-';

export const CONNECT_SCRIPT = 'source "$1"; dev_resolve && dev_assert_target && dev_assert_cached_identity && '
  + 'dev_run node -e \'console.log(JSON.stringify({url:process.env.CODETERM_API_URL,'
  + 'token:process.env.CODETERM_API_TOKEN,instance:process.env.CODETERM_INSTANCE}))\'';

export function deadlineError() {
  return Object.assign(new Error('bounded replay deadline elapsed'), {killed: true});
}

export function isolatedConnection(dev) {
  const port = Number(new URL(dev.url).port);
  if (!String(dev.instance ?? '').startsWith(ISOLATED_PREFIX) || !dev.token
    || port === PROD_PORT || port === SHARED_DEV_PORT) {
    throw new Error('Replay requires a named upgrade-restore-* DEV instance on its own port');
  }
  return dev;
}

export function devApi(dev, deadline, {fetch = globalThis.fetch, now = () => performance.now()} = {}) {
  isolatedConnection(dev);
  const encode = encodeURIComponent;
  async function api(path, method = 'GET') {
    const remaining = Math.ceil(deadline - now());
    if (remaining <= 0) throw deadlineError();
    let response;
    let text;
    try {
      response = await fetch(`${dev.url}${path}`, {method,
        headers: {Authorization: `Bearer ${dev.token}`}, signal: AbortSignal.timeout(remaining)});
      text = await response.text();
    } catch (error) {
      if (error?.name === 'TimeoutError' || error?.name === 'AbortError') throw deadlineError();
      throw error;
    }
    if (!response.ok) {
      throw Object.assign(new Error(`${method} ${path}: HTTP ${response.status} ${text.slice(0, 600)}`),
        {status: response.status});
    }
    return text ? JSON.parse(text) : null;
  }
  return {
    health: () => api('/api/health'),
    providers: () => api('/api/agents/providers'),
    focus: tab => api(`/api/tabs/${encode(tab)}/focus`, 'POST'),
    inspect: async tab => {
      const rows = await api(`/api/tabs/inspect?tab_ids=${encode(tab)}`);
      return Array.isArray(rows) ? rows[0] : rows;
    },
    chat: tab => api(`/api/tabs/${encode(tab)}/chat?since=0`),
  };
}
