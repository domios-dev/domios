import { devConnection } from "../_lib/connection.mjs";

export const topology = {
  A: { instance: "acceptance-r161", api: 7707, frontend: 15007, cdp: 9407 },
  B: { instance: "curated", api: 7697, frontend: 14997, cdp: 9397 },
  C: { instance: "curated-second", api: 7717, frontend: 15017, cdp: 9417 },
  D: { instance: "curated-third", api: 7727, frontend: 15027, cdp: 9427 },
};

export function requireThat(condition, description) {
  if (!condition) throw new Error(description);
}

export class Changes {
  epoch = 0;
  waiters = new Set();
  failure = null;
  pulse() {
    this.epoch++;
    for (const done of [...this.waiters]) done();
  }
  fail(error) { this.failure = error; this.pulse(); }
  async after(epoch, signal) {
    if (this.failure) throw this.failure;
    signal.throwIfAborted();
    if (this.epoch !== epoch) return;
    await new Promise((accept, reject) => {
      const abort = () => settle(reject, new Error("Observation deadline exceeded"));
      const settle = (done, value) => {
        this.waiters.delete(wake);
        signal.removeEventListener("abort", abort);
        done(value);
      };
      const wake = () => settle(accept);
      this.waiters.add(wake);
      signal.addEventListener("abort", abort, { once: true });
      if (signal.aborted) abort();
    });
    if (this.failure) throw this.failure;
  }
}

export class Peer {
  constructor(label) {
    this.label = label;
    this.target = topology[label];
    let connection;
    try { connection = devConnection({ instance: this.target.instance, apiPort: this.target.api }); }
    catch { throw new Error(`${label}: DEV helper resolution refused`); }
    requireThat(connection.api === `http://127.0.0.1:${this.target.api}`, `${label}: unexpected DEV address`);
    this.connection = connection;
  }
  async api(path, body, signal = AbortSignal.timeout(30_000)) {
    requireThat(path.startsWith("/api/"), "Only local API paths are allowed");
    const response = await fetch(this.connection.api + path, {
      method: body === undefined ? "GET" : "POST", redirect: "error", signal,
      headers: { Authorization: `Bearer ${this.connection.token}`, "Content-Type": "application/json" },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    });
    let value;
    try { value = await response.json(); } catch { throw new Error(`${this.label}: invalid API JSON`); }
    if (!response.ok) {
      const reason = typeof value?.code === "string" && /^[a-z_]{1,80}$/.test(value.code) ? value.code : "unavailable";
      throw new Error(`${this.label}: ${path} HTTP ${response.status} ${reason}`);
    }
    return value;
  }
  runtime(command) { return this.api("/api/account/neighbors/runtime", command); }
  async snapshot() {
    const [runtime, mux] = await Promise.all([this.runtime({ operation: "status" }), this.api("/api/mux/state")]);
    requireThat(mux.tabs && !Array.isArray(mux.tabs) && typeof mux.tabs === "object", `${this.label}: invalid mux snapshot`);
    const tabs = Object.values(mux.tabs);
    requireThat(tabs.length <= 512, `${this.label}: oversized tab snapshot`);
    return { runtime, tabs };
  }
  async open(changes) {
    this.stream = new AbortController();
    const response = await fetch(this.connection.api + "/api/events/stream", {
      redirect: "error", headers: { Authorization: `Bearer ${this.connection.token}` },
      signal: AbortSignal.any([this.stream.signal, AbortSignal.timeout(1_200_000)]),
    });
    requireThat(response.ok && response.body, `${this.label}: event stream unavailable`);
    const reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
    this.readEvents(reader, changes).catch(() => {
      if (!this.stream.signal.aborted) changes.fail(new Error(`${this.label}: event stream lost; observation incomplete`));
    });
  }
  async readEvents(reader, changes) {
    let buffer = "", count = 0;
    for (;;) {
      const { done, value } = await reader.read();
      requireThat(!done, "Event stream ended");
      buffer += value.replaceAll("\r", "");
      requireThat(buffer.length <= 4 * 1024 * 1024, "Event frame too large");
      let end;
      while ((end = buffer.indexOf("\n\n")) !== -1) {
        const frame = buffer.slice(0, end);
        buffer = buffer.slice(end + 2);
        const data = frame.split("\n").filter(line => line.startsWith("data:")).map(line => line.slice(5).trimStart()).join("\n");
        if (!data) continue;
        const event = JSON.parse(data);
        requireThat(event.event_type !== "overflow", "Event replay overflow");
        requireThat(++count <= 100_000, "Event observation bound exceeded");
        if (/^(neighbor-|mux-state-changed$|agent-|tab-|tabs-)/.test(event.event_type)) changes.pulse();
      }
    }
  }
  close() { this.stream?.abort(); }
}

export async function observe(changes, read, ready, seconds) {
  const signal = AbortSignal.timeout(seconds * 1000);
  for (;;) {
    signal.throwIfAborted();
    const epoch = changes.epoch;
    if (changes.failure) throw changes.failure;
    const value = await read();
    signal.throwIfAborted();
    if (changes.failure) throw changes.failure;
    if (ready(value)) return value;
    await changes.after(epoch, signal);
  }
}

export function friend(snapshot, id) { return snapshot.runtime.friend_receivers?.[id]; }
export function authorityTabs(snapshot, id) {
  return snapshot.tabs.filter(tab => tab.neighbor_authority?.friendship_id === id);
}
export function root(snapshot, id) {
  const tabs = authorityTabs(snapshot, id).filter(tab => !tab.neighbor_authority.root_tab_id);
  return tabs.length === 1 ? tabs[0] : null;
}
export function receiving(snapshot, id, here, source) {
  const state = friend(snapshot, id);
  return state?.receiving_enabled === true && state.receiving_here === here && state.source === source;
}
export function ready(snapshot, id) {
  const tab = root(snapshot, id);
  return snapshot.runtime.concierges?.[id]?.state === "ready" &&
    snapshot.runtime.bindings?.[id] === tab?.id && Boolean(tab?.agent_session_id) &&
    Boolean(tab?.session_binding_nonce) && tab.session_binding_nonce === tab.reported_session_binding_nonce;
}
export function retired(snapshot, id) {
  return !snapshot.runtime.bindings?.[id] && !snapshot.runtime.concierges?.[id]?.gateway_tab &&
    snapshot.runtime.concierges?.[id]?.concierge_dormant === true && authorityTabs(snapshot, id).length === 0;
}
