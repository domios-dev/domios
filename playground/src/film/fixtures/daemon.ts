import "./clock";
import { baseDaemonTransport, configureDaemonTransport } from "../../../../src/lib/daemonBootstrap";
import { configureOrchestrationTransport } from "../../../../src/lib/transport/orchestrationRuntime";

type Route = (url: URL, body: unknown) => unknown;

const ORIGIN = window.location.origin;
const routes = new Map<string, Route>();
const unrouted = new Set<string>();

/** Answers the film daemon's `METHOD /api/...` with the route's JSON. */
export function filmRoute(method: string, path: string, route: Route): void {
  routes.set(`${method} ${path}`, route);
}

/** Answers one request from the film's route table, for requests that arrive wrapped (the mesh relay). */
export async function answerFilmRoute(method: string, path: string, body: unknown): Promise<{ found: boolean; value: unknown }> {
  const url = new URL(path, ORIGIN);
  const route = routes.get(`${method.toUpperCase()} ${url.pathname}`);
  if (!route) { unrouted.add(`${method.toUpperCase()} ${url.pathname} (relay)`); return { found: false, value: null }; }
  return { found: true, value: await route(url, body) };
}

declare global {
  interface Window {
    __filmUnrouted?: () => string[];
  }
}

window.__filmUnrouted = () => [...unrouted];

const json = (value: unknown, status = 200) => new Response(JSON.stringify(value ?? null), { status, headers: { "Content-Type": "application/json" } });
// A daemon event stream that stays open and quiet: films never depend on pushed events.
const quietStream = () => new Response(new ReadableStream({ start: () => undefined }), { status: 200, headers: { "Content-Type": "text/event-stream" } });

const priorFetch = window.fetch;
window.fetch = async (input, init) => {
  const url = new URL(input instanceof Request ? input.url : String(input), ORIGIN);
  if (url.origin !== ORIGIN || !url.pathname.startsWith("/api/")) return priorFetch(input, init);
  const method = (init?.method ?? "GET").toUpperCase();
  if (new Headers(init?.headers).get("Accept") === "text/event-stream") return quietStream();
  const route = routes.get(`${method} ${url.pathname}`);
  if (!route) {
    unrouted.add(`${method} ${url.pathname}`);
    return json({ error: "not in the film fixtures" }, 404);
  }
  const body = typeof init?.body === "string" && init.body ? JSON.parse(init.body) : undefined;
  return json(await route(url, body));
};

filmRoute("POST", "/api/frontend/diagnostic", () => ({}));
filmRoute("GET", "/api/plugins/tab-menu", () => ({ items: [], receiverTargets: {} }));

// The film page is its own daemon: the shipped transport talks to the page origin and every /api read lands on a fixture route.
configureDaemonTransport({ origin: ORIGIN, token: "film" });
configureOrchestrationTransport(baseDaemonTransport());
