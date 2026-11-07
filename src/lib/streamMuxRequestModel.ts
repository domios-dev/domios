export type MuxTraffic = "interactive" | "bulk";
export interface MuxRequestPolicy { traffic: MuxTraffic; cancellable: boolean }
export const MUX_REQUEST_TIMEOUT = Symbol("mux-request-timeout");
export type MuxRequestInit = RequestInit & { [MUX_REQUEST_TIMEOUT]?: number | null };

const READ_POST = /^\/api\/(?:tabs\/[^/]+\/(?:preview\/(?:read|check)|chat-raw\/(?:start|history|backfill))|essence-maps)$/;
const BULK_GET = /^\/api\/(?:essence-source|logs|sessions\/transcript|agent-runs\/[^/]+\/log|mem\/workers\/[^/]+\/log)$/;

export function muxRequestOperation(method: string, path: string, body?: BodyInit | null): { method: string; path: string } {
  const verb = method.toUpperCase();
  const pathname = path.split("?")[0];
  if (verb === "POST" && pathname === "/api/mesh/relay-to" && typeof body === "string" && body.length <= 64 * 1024) {
    try {
      const relay: unknown = JSON.parse(body);
      if (relay && typeof relay === "object" && "method" in relay && typeof relay.method === "string"
        && "path" in relay && typeof relay.path === "string" && !relay.path.startsWith("/api/mesh/relay-to")) {
        return { method: relay.method.toUpperCase(), path: relay.path };
      }
    } catch {}
  }
  return { method: verb, path };
}

export function muxRequestPolicy(method: string, path: string, body?: BodyInit | null): MuxRequestPolicy {
  const operation = muxRequestOperation(method, path, body);
  const verb = operation.method, pathname = operation.path.split("?")[0];
  const read = verb === "GET" || verb === "HEAD";
  const readPost = verb === "POST" && READ_POST.test(pathname);
  return { traffic: readPost || read && BULK_GET.test(pathname) ? "bulk" : "interactive", cancellable: read || readPost };
}

export function muxPostTraffic(traffic: MuxTraffic, upload: boolean): MuxTraffic { return upload ? "bulk" : traffic; }

export function muxRequestAdmission(stream: boolean, traffic: MuxTraffic, streams: number, requests: number, bulk: number, foreground = false): boolean {
  return stream ? streams < 256 : foreground || requests < 48 && (traffic !== "bulk" || bulk < 48);
}

export interface MuxPostState {
  readonly active: ReadonlyMap<string, { traffic: MuxTraffic; foreground: boolean }>;
  readonly pending: ReadonlyArray<{ id: string; traffic: MuxTraffic; foreground: boolean }>;
  readonly burst: number;
}
export const EMPTY_MUX_POSTS: MuxPostState = { active: new Map(), pending: [], burst: 0 };

export function muxPostEnqueue(state: MuxPostState, id: string, traffic: MuxTraffic, foreground = false): MuxPostState {
  return { ...state, pending: [...state.pending, { id, traffic, foreground }] };
}

export function muxPostRemove(state: MuxPostState, id: string): MuxPostState {
  const active = new Map(state.active); active.delete(id);
  return { ...state, active, pending: state.pending.filter(item => item.id !== id) };
}

export function muxPostCancel(state: MuxPostState, id: string): MuxPostState {
  return { ...state, pending: state.pending.filter(item => item.id !== id) };
}

export function muxPostStart(state: MuxPostState, canStart: (id: string, selected: string[]) => boolean = () => true): { state: MuxPostState; ids: string[] } {
  const active = new Map(state.active), pending = [...state.pending], ids: string[] = [];
  let burst = state.burst;
  while (active.size < 4) {
    const bulkAvailable = [...active.values()].filter(item => item.traffic === "bulk").length < 2;
    const eligible = (item: MuxPostState["pending"][number]) => (item.traffic !== "bulk" || bulkAvailable) && canStart(item.id, ids);
    const foreground = pending.findIndex(item => item.foreground && eligible(item));
    const interactive = active.size < 3 ? pending.findIndex(item => !item.foreground && item.traffic === "interactive" && eligible(item)) : -1;
    const bulk = active.size < 3 ? pending.findIndex(item => !item.foreground && item.traffic === "bulk" && eligible(item)) : -1;
    const index = foreground >= 0 ? foreground : interactive >= 0 && (burst < 8 || bulk < 0) ? interactive : bulk;
    if (index < 0) break;
    const [item] = pending.splice(index, 1);
    active.set(item.id, { traffic: item.traffic, foreground: item.foreground }); ids.push(item.id);
    burst = item.traffic === "interactive" ? Math.min(8, burst + 1) : 0;
  }
  return { state: { active, pending, burst }, ids };
}

export type MuxResponseStage = "head" | "body" | "done";
export type MuxRequestPhase = "registered" | "posted" | "acked" | MuxResponseStage | "cancelled" | "lost";
export interface MuxRequestState {
  readonly phase: MuxRequestPhase;
  readonly cancellable: boolean;
  readonly reposts: number;
  readonly early: MuxResponseStage | null;
}
export type MuxRequestEvent =
  | { type: "posted" | "acked" | "cancel" | "lost" }
  | { type: "refused"; status: number }
  | { type: "frame"; stage: MuxResponseStage };
export type MuxRequestAction = "none" | "buffer" | "deliver" | "reopen" | "not-dispatched" | "retryable" | "outcome-unknown" | "cancel-read" | "detach-mutation";
export interface MuxRequestStep { state: MuxRequestState; action: MuxRequestAction }

export function initialMuxRequest(cancellable: boolean): MuxRequestState {
  return { phase: "registered", cancellable, reposts: 0, early: null };
}

export function transitionMuxRequest(state: MuxRequestState, event: MuxRequestEvent): MuxRequestStep {
  if (["done", "cancelled", "lost"].includes(state.phase)) return { state, action: "none" };
  const next = (phase: MuxRequestPhase, action: MuxRequestAction): MuxRequestStep => ({ state: { ...state, phase }, action });
  switch (event.type) {
    case "posted": return state.phase === "registered" ? next("posted", "none") : { state, action: "none" };
    case "acked": return state.phase === "posted" ? next(state.early ?? "acked", state.early ? "deliver" : "none") : { state, action: "none" };
    case "frame": return state.phase === "posted"
      ? { state: { ...state, early: event.stage }, action: "buffer" }
      : next(event.stage, "deliver");
    case "refused":
      if (state.phase !== "posted") return next("lost", state.cancellable ? "retryable" : "outcome-unknown");
      if (![400, 403, 409, 410, 413, 429].includes(event.status)) return next("lost", state.cancellable ? "retryable" : "outcome-unknown");
      if (event.status === 410 && state.reposts === 0) {
        return { state: { ...state, phase: "registered", reposts: 1, early: null }, action: "reopen" };
      }
      return next("lost", "not-dispatched");
    case "cancel": return next("cancelled", state.phase === "registered" ? "none" : state.cancellable ? "cancel-read" : "detach-mutation");
    case "lost": return next("lost", state.phase === "registered" ? "not-dispatched" : state.cancellable ? "retryable" : "outcome-unknown");
  }
}
