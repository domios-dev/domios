import { uid } from "./uid";
import { browserFetch, createTransportClient, SseCapacityError, streamClientId, TransportError, type TransportClient, type TransportOptions, type Unsubscribe } from "./transportClient";
import { bulkCreditDue, CREDIT_ATTEMPTS, decodeMuxFrame, isDirectDaemonPath, isMuxStreamPath, muxSessionRefused, type MuxFrame } from "./streamMuxModel";
import { createMuxReadCapacity, EMPTY_MUX_POSTS, initialMuxRequest, MUX_READ_CAPACITY, MUX_REQUEST_TIMEOUT, muxPostCancel, muxPostEnqueue, muxPostRemove, muxPostStart, muxPostTraffic, muxRequestAdmission, muxRequestOperation, muxRequestPolicy, transitionMuxRequest, type MuxPostState, type MuxRequestInit, type MuxRequestState, type MuxTraffic } from "./streamMuxRequestModel";
import { restRequestTimeoutMs } from "./restTimeoutPolicy";
import { RECONNECT_FIRST_MS, reconnectDelay, nextReconnectBackoff, shouldResetReconnectBackoff } from "./webReconnectModel";

type FetchPort = NonNullable<TransportOptions["fetch"]>;
type Head = Extract<MuxFrame, { kind: "open" }>;
const SESSION_BUFFER = 4 * 1024 * 1024;
const REQUEST_BUFFER = 512 * 1024;

function responseBufferLimit(head: Head): number {
  const length = head.headers.find(([name]) => name.toLowerCase() === "content-length")?.[1];
  if (length == null || !/^\d+$/.test(length)) return REQUEST_BUFFER;
  const bytes = Number(length);
  return Number.isSafeInteger(bytes) ? Math.min(SESSION_BUFFER, Math.max(REQUEST_BUFFER, bytes)) : REQUEST_BUFFER;
}

class MuxCapacityError extends SseCapacityError {
  constructor(scope: "frame" | "session" | "per-request", bytes: number, limit: number) {
    super(bytes);
    this.name = "MuxCapacityError";
    this.message = `Mux ${scope} buffer limit exceeded (${bytes} bytes; limit ${limit} bytes)`;
  }
}

export class OutcomeUnknown extends Error {
  constructor(public method: string, public path: string, cause?: string) {
    super("The daemon request may have completed; reconcile its status before sending again" + (cause ? ` (${cause})` : ""));
    this.name = "OutcomeUnknown";
  }
}
export class RetryableMuxError extends TransportError {
  readonly retryable = true;
  constructor(path: string, message: string) { super(0, path, message); this.name = "RetryableTransportError"; }
}

export class MuxAdmissionError extends RetryableMuxError {
  readonly dispatched = false;
  readonly code = "client_capacity";
  readonly [MUX_READ_CAPACITY]?: (signal?: AbortSignal) => Promise<void>;
  constructor(path: string, capacity?: (signal?: AbortSignal) => Promise<void>) {
    super(path, "client_capacity: Other requests are using the available connection capacity; this request was not sent.");
    this[MUX_READ_CAPACITY] = capacity;
  }
}

// Body readers rethrow the typed carrier outcome; runtimes may wrap stream errors.
class MuxResponse extends Response {
  constructor(body: BodyInit | null, init: ResponseInit, private readonly outcome: () => unknown) { super(body, init); }
  private typed<T>(read: Promise<T>): Promise<T> { return read.catch((error: unknown) => { throw this.outcome() ?? error; }); }
  override text(): Promise<string> { return this.typed(super.text()); }
  override json(): Promise<any> { return this.typed(super.json()); }
  override arrayBuffer(): Promise<ArrayBuffer> { return this.typed(super.arrayBuffer()); }
  override blob(): Promise<Blob> { return this.typed(super.blob()); }
}

interface Pending {
  id: string;
  outcome: unknown;
  path: string;
  method: string;
  init: MuxRequestInit;
  stream: boolean;
  traffic: MuxTraffic;
  postTraffic: MuxTraffic;
  foreground: boolean;
  state: MuxRequestState;
  session: string | null;
  acked: boolean;
  head: Head | null;
  resolved: boolean;
  ended: boolean;
  queue: Uint8Array[];
  buffered: number;
  bufferLimit: number;
  tracked: boolean;
  demand: boolean;
  controller: ReadableStreamDefaultController<Uint8Array> | null;
  aborter: AbortController;
  post: Promise<void> | null;
  detach: () => void;
  resolve: (response: Response) => void;
  reject: (error: unknown) => void;
}

export interface StreamMux { fetch: FetchPort; close: () => void; wake: () => void; idle: () => Promise<void>; busy: () => boolean }

export function createStreamMux(transport: () => TransportClient, fetcher: FetchPort): StreamMux {
  const pending = new Map<string, Pending>();
  const waiting = new Map<string, { resolve: (session: string) => void; reject: (error: unknown) => void }>();
  const posts = new Map<string, () => Promise<void>>();
  const readCapacity = createMuxReadCapacity();
  let postState: MuxPostState = EMPTY_MUX_POSTS;
  let current: string | null = null;
  let live: string | null = null;
  let closeStream: Unsubscribe | null = null;
  let retry: ReturnType<typeof setTimeout> | undefined;
  let backoff = RECONNECT_FIRST_MS;
  let buffered = 0;
  let closed = false;

  function kick(): void {
    const requests = [...pending.values()].filter(entry => !entry.stream && entry.state.phase !== "registered").length;
    const next = muxPostStart(postState, (id, selected) => {
      const entry = pending.get(id);
      if (!entry) return false;
      if (entry.stream) return true;
      const reserved = selected.filter(key => !pending.get(key)?.stream).length;
      return requests + reserved < (entry.foreground ? 64 : 48);
    }); postState = next.state;
    for (const id of next.ids) {
      const post = posts.get(id); posts.delete(id);
      if (post) void post();
      else postState = muxPostRemove(postState, id);
    }
  }

  function releaseBuffer(entry: Pending): void {
    if (entry.tracked) buffered -= entry.buffered;
    entry.buffered = 0; entry.queue = [];
  }

  function cleanup(entry: Pending): void {
    entry.detach();
    if (entry.tracked) { buffered -= entry.buffered; entry.tracked = false; }
    pending.delete(entry.id);
    posts.delete(entry.id);
    postState = muxPostCancel(postState, entry.id);
    const waiter = waiting.get(entry.id); waiting.delete(entry.id);
    waiter?.reject(entry.aborter.signal.reason ?? new Error("Mux request closed before dispatch"));
    if (!pending.size && !live && current) {
      const close = closeStream; current = null; closeStream = null; close?.();
    }
    // After the caller's drain, so an idle waiter sees complete bodies.
    queueMicrotask(() => {
      kick(); notifyIdle();
      if (!closed && muxRequestAdmission(false, "interactive", 0, [...pending.values()].filter(item => !item.stream).length, 0)) readCapacity.released();
    });
  }

  const idleWaiters: Array<() => void> = [];
  function requestsInFlight(): boolean { return [...pending.values()].some(entry => !entry.stream); }
  function notifyIdle(): void { if (!requestsInFlight()) for (const waiter of idleWaiters.splice(0)) waiter(); }

  function failure(entry: Pending, error?: unknown): void {
    if (!pending.has(entry.id)) return;
    const step = transitionMuxRequest(entry.state, { type: "lost" });
    entry.state = step.state;
    const reason = error instanceof SseCapacityError && entry.stream ? error
      : error instanceof TransportError && muxSessionRefused(error.status) ? error
      : !entry.state.cancellable && entry.session !== null && step.action !== "not-dispatched" ? new OutcomeUnknown(entry.method, entry.path, error instanceof Error ? error.message : undefined)
      : new RetryableMuxError(entry.path, error instanceof Error ? error.message : "Mux connection lost");
    entry.outcome = reason;
    cleanup(entry); releaseBuffer(entry);
    entry.aborter.abort(reason);
    entry.reject(reason);
    try { entry.controller?.error(reason); } catch {}
  }

  async function control(path: string, init: RequestInit): Promise<Response> {
    const client = transport(), headers = new Headers(init.headers);
    const token = client.currentToken();
    if (!token) throw new Error("Daemon bearer token required");
    headers.set("Authorization", "Bearer " + token);
    headers.set("X-CodeTerm-Client-Id", streamClientId);
    return fetcher(client.origin() + path, { ...init, headers, credentials: "omit" });
  }

  function cancelRemote(entry: Pending): void {
    const owner = entry.session;
    if (!owner) return;
    void (entry.post ?? Promise.resolve()).then(async () => {
      if (closed) return;
      const aborter = new AbortController();
      const timer = setTimeout(() => aborter.abort(), 15_000);
      try {
        const path = entry.stream ? "/channels/" : "/requests/";
        const response = await control("/api/stream-mux/" + owner + path + entry.id, { method: "DELETE", signal: aborter.signal });
        await response.body?.cancel();
      } finally { clearTimeout(timer); }
    }).catch(() => {});
  }

  function cancel(entry: Pending, reason: unknown): void {
    const previous = entry.state;
    const step = transitionMuxRequest(previous, { type: "cancel" });
    entry.state = step.state;
    if (step.action === "cancel-read" || step.action === "detach-mutation") cancelRemote(entry);
    const rejection = step.action === "detach-mutation" ? new OutcomeUnknown(entry.method, entry.path)
      : previous.phase === "registered" && posts.has(entry.id) && reason instanceof DOMException && reason.name === "TimeoutError"
        ? new MuxAdmissionError(entry.path) : reason;
    entry.outcome = rejection;
    cleanup(entry); releaseBuffer(entry); entry.aborter.abort(reason);
    entry.reject(rejection);
    try { entry.controller?.error(rejection); } catch {}
    kick();
  }

  function drain(entry: Pending): void {
    if (!entry.acked || !entry.controller) return;
    if (entry.demand && entry.queue.length) {
      const bytes = entry.queue.shift()!;
      if (entry.tracked) buffered -= bytes.byteLength;
      entry.buffered -= bytes.byteLength;
      entry.demand = false; entry.controller.enqueue(bytes);
    }
    if (entry.ended && entry.queue.length === 0) {
      try { entry.controller.close(); } catch {}
    }
  }

  function deliverHead(entry: Pending): void {
    if (!entry.acked || !entry.head || entry.resolved) return;
    const body = new ReadableStream<Uint8Array>({
      start(controller) { entry.controller = controller; },
      pull() { entry.demand = true; drain(entry); },
      cancel(reason) { cancel(entry, reason); },
    }, { highWaterMark: 0 });
    const nullBody = entry.method === "HEAD" || [204, 205, 304].includes(entry.head.status);
    const response = new MuxResponse(nullBody ? null : body, { status: entry.head.status, headers: entry.head.headers }, () => entry.outcome);
    entry.resolved = true;
    entry.resolve(response);
    drain(entry);
  }

  function finish(entry: Pending, end: Extract<MuxFrame, { kind: "end" }>["end"]): void {
    if (end.reason !== "eof") {
      const error = end.reason === "capacity" ? new SseCapacityError(end.bytes) : new Error(end.message);
      cancelRemote(entry); failure(entry, error); return;
    }
    entry.ended = true;
    deliverHead(entry);
    if (!entry.resolved) { failure(entry, new Error("Mux ended without response headers")); return; }
    cleanup(entry); drain(entry);
  }

  function route(value: unknown): void {
    const frame = decodeMuxFrame(value);
    if (!frame) throw new Error("Invalid mux response frame");
    countCredit(frame);
    const entry = pending.get(frame.channel);
    if (!entry) return;
    if (frame.kind === "open") {
      if (entry.head) { failure(entry, new Error("Duplicate mux response head")); return; }
      entry.head = frame;
      entry.bufferLimit = responseBufferLimit(frame);
      entry.state = transitionMuxRequest(entry.state, { type: "frame", stage: "head" }).state;
      deliverHead(entry);
    } else if (frame.kind === "chunk") {
      if (!entry.head) { failure(entry, new Error("Mux body preceded response headers")); return; }
      let bytes: Uint8Array;
      try {
        bytes = frame.enc === "utf8" ? new TextEncoder().encode(frame.chunk)
          : Uint8Array.from(atob(frame.chunk), character => character.charCodeAt(0));
      } catch (error) { failure(entry, error); return; }
      const overflow = bytes.byteLength > 64 * 1024 ? new MuxCapacityError("frame", bytes.byteLength, 64 * 1024)
        : buffered + bytes.byteLength > SESSION_BUFFER ? new MuxCapacityError("session", buffered + bytes.byteLength, SESSION_BUFFER)
        : entry.buffered + bytes.byteLength > entry.bufferLimit ? new MuxCapacityError("per-request", entry.buffered + bytes.byteLength, entry.bufferLimit) : null;
      if (overflow) {
        cancelRemote(entry); failure(entry, overflow); return;
      }
      entry.state = transitionMuxRequest(entry.state, { type: "frame", stage: "body" }).state;
      entry.queue.push(bytes); entry.buffered += bytes.byteLength; buffered += bytes.byteLength;
      drain(entry);
    } else {
      if (frame.end.reason === "eof") entry.state = transitionMuxRequest(entry.state, { type: "frame", stage: "done" }).state;
      entry.ended = true;
      if (entry.acked) finish(entry, frame.end);
      else earlyEnds.set(entry.id, frame.end);
    }
  }

  const earlyEnds = new Map<string, Extract<MuxFrame, { kind: "end" }>["end"]>();

  let credit = { session: "", received: 0, posted: 0 };

  function countCredit(frame: MuxFrame): void {
    if (frame.kind !== "chunk" || frame.credit === 0 || !live || credit.session !== live) return;
    credit.received += frame.credit;
    const due = bulkCreditDue(credit.received, credit.posted);
    if (due === null) return;
    credit.posted = due;
    postCredit(credit.session, due, 1);
  }

  // A closed window sends no further credited frames, so a lost credit is re-posted here; it is cumulative and idempotent.
  function postCredit(session: string, received: number, attempt: number): void {
    void control("/api/stream-mux/" + session + "/credit", {
      method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ received }),
    }).then(response => { void response.body?.cancel(); return response.ok; }, () => false).then(ok => {
      if (ok || credit.session !== session || live !== session || attempt >= CREDIT_ATTEMPTS) return;
      postCredit(session, credit.posted, attempt + 1);
    });
  }

  function refused(owner: string, status: number): void {
    if (current !== owner) return;
    current = null; live = null; closeStream = null;
    const error = new TransportError(status, "/api/stream-mux", "Daemon refused this client's credentials");
    const waiters = [...waiting.values()]; waiting.clear();
    for (const waiter of waiters) waiter.reject(error);
    for (const entry of [...pending.values()]) if (entry.state.phase !== "registered") failure(entry, error);
  }

  function lost(owner: string, error?: unknown): void {
    if (current !== owner) return;
    current = null; live = null; closeStream = null;
    for (const entry of [...pending.values()]) if (entry.state.phase !== "registered") failure(entry, error);
    if (!closed) {
      const delay = reconnectDelay(backoff, Math.random());
      backoff = nextReconnectBackoff(backoff);
      retry = setTimeout(() => { retry = undefined; if (waiting.size) open(); }, delay);
    }
  }

  function open(): void {
    if (closed || current || retry) return;
    const owner = uid(), started = Date.now();
    let sawFrame = false;
    current = owner;
    closeStream = transport().subscribe("/api/stream-mux?session=" + owner, (_name, value) => {
      if (current !== owner) return;
      sawFrame = true; route(value);
    }, {
      raw: true, requireEventStream: true, local: true,
      onReady() {
        if (current !== owner) return;
        live = owner;
        credit = { session: owner, received: 0, posted: 0 };
        const waiters = [...waiting.values()]; waiting.clear();
        for (const waiter of waiters) waiter.resolve(owner);
      },
      onEnd(end) {
        if (end.reason === "http" && end.status !== undefined && muxSessionRefused(end.status)) { refused(owner, end.status); return; }
        if (shouldResetReconnectBackoff(sawFrame, Date.now() - started)) backoff = RECONNECT_FIRST_MS;
        lost(owner, end.error);
      },
    });
  }

  function ready(entry: Pending): Promise<string> {
    if (live) return Promise.resolve(live);
    return new Promise((resolve, reject) => { waiting.set(entry.id, { resolve, reject }); open(); });
  }

  async function prepare(entry: Pending): Promise<void> {
    try {
      const owner = await ready(entry);
      if (!pending.has(entry.id)) return;
      entry.session = owner;
      posts.set(entry.id, () => post(entry));
      postState = muxPostEnqueue(postState, entry.id, entry.postTraffic, entry.foreground); kick();
    } catch (error) { failure(entry, error); }
  }

  async function post(entry: Pending): Promise<void> {
    let reopen = false;
    try {
      if (!pending.has(entry.id)) return;
      if (entry.session !== live) { reopen = true; return; }
      entry.state = transitionMuxRequest(entry.state, { type: "posted" }).state;
      const headers = new Headers(entry.init.headers);
      let body = entry.init.body;
      let path = "/api/stream-mux/" + entry.session + "/requests/" + entry.id;
      if (entry.stream) {
        path = "/api/stream-mux/" + entry.session + "/channels";
        headers.set("Content-Type", "application/json"); headers.delete("Accept");
        body = JSON.stringify({ channel: entry.id, path: entry.path });
      } else {
        headers.set("X-Domios-Mux-Method", entry.method); headers.set("X-Domios-Mux-Path", entry.path);
        headers.set("X-Domios-Mux-Class", entry.traffic);
        if (entry.state.cancellable) headers.set("X-Domios-Mux-Cancellable", "1");
      }
      const attempted = control(path, { method: "POST", headers, body, signal: entry.aborter.signal });
      entry.post = attempted.then(() => {}, () => {});
      const ack = await attempted;
      if (!pending.has(entry.id)) { await ack.body?.cancel(); return; }
      if (ack.status !== 202) {
        const step = transitionMuxRequest(entry.state, { type: "refused", status: ack.status });
        entry.state = step.state;
        if (step.action === "retryable" || step.action === "outcome-unknown") {
          await ack.body?.cancel(); throw new Error("Mux acknowledgement failed: HTTP " + ack.status);
        }
        if (step.action === "reopen") {
          await ack.body?.cancel();
          const close = closeStream;
          const owner = current;
          if (owner) lost(owner, new Error("Mux session no longer registered"));
          close?.(); entry.head = null; releaseBuffer(entry); earlyEnds.delete(entry.id);
          reopen = true;
        } else {
          const bytes = await ack.arrayBuffer();
          cleanup(entry); releaseBuffer(entry);
          entry.resolve(new Response(bytes, { status: ack.status, headers: ack.headers }));
        }
        return;
      }
      const accepted: unknown = await ack.json();
      if (!accepted || typeof accepted !== "object" || !("accepted" in accepted) || accepted.accepted !== true) throw new Error("Invalid mux acknowledgement");
      if (!pending.has(entry.id)) return;
      entry.state = transitionMuxRequest(entry.state, { type: "acked" }).state;
      entry.acked = true; deliverHead(entry);
      const end = earlyEnds.get(entry.id); earlyEnds.delete(entry.id);
      if (end) finish(entry, end);
    } catch (error) {
      // An unreadable request body fails inside the browser before any byte is sent: provably not dispatched.
      if (error instanceof DOMException && error.name === "NotReadableError" && !entry.acked) {
        entry.outcome = error; cleanup(entry); releaseBuffer(entry); entry.reject(error); return;
      }
      if (current === entry.session) cancelRemote(entry);
      failure(entry, error);
    } finally {
      postState = muxPostRemove(postState, entry.id); kick();
      if (reopen && pending.has(entry.id)) void prepare(entry);
    }
  }

  return {
    fetch(input, init = {}) {
      const client = transport(), url = new URL(input instanceof Request ? input.url : String(input), client.origin());
      if (url.origin !== client.origin()) return Promise.reject(new Error("Mux requires its own daemon origin"));
      const path = url.pathname + url.search;
      if (isDirectDaemonPath(path)) return fetcher(input, init);
      const method = (init.method ?? (input instanceof Request ? input.method : "GET")).toUpperCase();
      const stream = method === "GET" && isMuxStreamPath(path);
      const headers = new Headers(input instanceof Request ? input.headers : undefined);
      new Headers(init.headers).forEach((value, key) => headers.set(key, value));
      const options: MuxRequestInit = { ...init, headers, signal: init.signal ?? (input instanceof Request ? input.signal : undefined), body: init.body ?? (input instanceof Request ? input.body : undefined) };
      const operation = muxRequestOperation(method, path, options.body);
      const policy = muxRequestPolicy(method, path, options.body);
      const foreground = !policy.cancellable || operation.method === "POST" && /^\/api\/tabs\/[^/]+\/chat-raw\/start$/.test(operation.path.split("?")[0]);
      const postTraffic = muxPostTraffic(policy.traffic, options.body != null && (typeof options.body !== "string" || options.body.length > 64 * 1024));
      const entries = [...pending.values()];
      const streams = entries.filter(entry => entry.stream).length;
      const bulk = entries.filter(entry => !entry.stream && entry.postTraffic === "bulk").length;
      if (closed) return Promise.reject(new RetryableMuxError(path, "Mux connection closed before dispatch"));
      if (!muxRequestAdmission(stream, postTraffic, streams, entries.length - streams, bulk, foreground)) {
        return Promise.reject(new MuxAdmissionError(path, !stream && ["GET", "HEAD"].includes(operation.method) ? readCapacity.capture() : undefined));
      }
      return new Promise<Response>((resolve, reject) => {
        const aborter = new AbortController();
        const entry: Pending = {
          id: uid(), method, path, init: options, stream, traffic: policy.traffic,
          postTraffic, foreground,
          state: initialMuxRequest(stream || policy.cancellable), session: null, acked: false, head: null,
          outcome: undefined, resolved: false, ended: false, queue: [], buffered: 0, bufferLimit: REQUEST_BUFFER, tracked: true, demand: false, controller: null,
          aborter, post: null, detach: () => {}, resolve, reject,
        };
        const timeout = stream ? null : restRequestTimeoutMs(operation.method, operation.path, options[MUX_REQUEST_TIMEOUT]);
        const timer = timeout === null ? undefined : setTimeout(() => cancel(entry, new DOMException("The operation timed out.", "TimeoutError")), timeout);
        const abort = () => cancel(entry, options.signal?.reason ?? new DOMException("Aborted", "AbortError"));
        entry.detach = () => { clearTimeout(timer); options.signal?.removeEventListener("abort", abort); earlyEnds.delete(entry.id); };
        pending.set(entry.id, entry);
        if (options.signal?.aborted) { abort(); return; }
        options.signal?.addEventListener("abort", abort, { once: true });
        void prepare(entry);
      });
    },
    busy: requestsInFlight,
    idle() {
      return requestsInFlight() ? new Promise<void>(resolve => { idleWaiters.push(resolve); }) : Promise.resolve();
    },
    // A wake (network back, page visible, explicit retry) skips the remaining backoff once.
    wake() {
      if (closed || current || !retry) return;
      clearTimeout(retry); retry = undefined;
      if (waiting.size) open();
    },
    close() {
      closed = true; clearTimeout(retry);
      readCapacity.close(new DOMException("Mux origin closed", "AbortError"));
      const owner = current, close = closeStream;
      if (owner) lost(owner, new Error("Mux origin closed"));
      for (const entry of [...pending.values()]) cancel(entry, new DOMException("Mux origin closed", "AbortError"));
      close?.(); closeStream = null;
    },
  };
}

interface Holder { client: TransportClient; fetcher: FetchPort; mux: StreamMux; token: TransportOptions["token"] }
const muxesByOrigin = new Map<string, Holder>();

export function withStreamMux(options: TransportOptions): TransportClient {
  const fetcher = options.fetch ?? browserFetch;
  const client = createTransportClient({ ...options, relayPeerId: undefined, fetch: fetcher });
  let holder = muxesByOrigin.get(client.origin());
  if (holder && holder.token !== options.token) { holder.mux.close(); muxesByOrigin.delete(client.origin()); holder = undefined; }
  if (holder) { holder.client = client; holder.fetcher = fetcher; }
  else {
    const created: Holder = { client, fetcher, token: options.token, mux: null! };
    created.mux = createStreamMux(() => created.client, (input, init) => created.fetcher(input, init));
    muxesByOrigin.set(client.origin(), created); holder = created;
  }
  return createTransportClient({ ...options, fetch: holder.mux.fetch });
}

export function wakeStreamMuxes(): void {
  for (const holder of muxesByOrigin.values()) holder.mux.wake();
}

/** Resolves once no carrier request is in flight and every finished response body is complete. */
export async function __streamMuxesIdleForTest(): Promise<void> {
  await Promise.all([...muxesByOrigin.values()].map(holder => holder.mux.idle()));
}

export function __streamMuxesBusyForTest(): boolean {
  return [...muxesByOrigin.values()].some(holder => holder.mux.busy());
}

export function __resetStreamMuxesForTest(): void {
  for (const holder of muxesByOrigin.values()) holder.mux.close();
  muxesByOrigin.clear();
}
