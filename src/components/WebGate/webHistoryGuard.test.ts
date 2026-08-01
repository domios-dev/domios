import { installWebHistoryGuard } from "./webHistoryGuard";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const listeners = new Map<string, EventListener>();
const calls: Array<{ kind: "replace" | "push"; state: unknown; url: string }> = [];
const history = {
  state: null as unknown,
  replaceState(state: unknown, _unused: string, url: string) {
    this.state = state;
    calls.push({ kind: "replace", state, url });
  },
  pushState(state: unknown, _unused: string, url: string) {
    this.state = state;
    calls.push({ kind: "push", state, url });
  },
};

Object.defineProperty(globalThis, "window", {
  configurable: true,
  value: {
    history,
    location: { pathname: "/app", search: "?mode=web", hash: "" },
    addEventListener(type: string, listener: EventListener) {
      listeners.set(type, listener);
    },
    removeEventListener(type: string) {
      listeners.delete(type);
    },
  },
});

const remove = installWebHistoryGuard();
assert(calls.length === 2, "guard creates one root and one sentinel entry");
assert(calls[0]?.kind === "replace" && calls[1]?.kind === "push", "guard preserves the app URL before arming");

listeners.get("popstate")?.({ state: { codetermWebHistoryGuard: "root" } } as PopStateEvent);
assert(Number(calls.length) === 3 && calls[2]?.kind === "push", "browser Back restores the sentinel without leaving the app");

remove();
assert(!listeners.has("popstate"), "guard removes its listener");

console.log("✓ web history guard keeps the CodeTerm shell mounted");
