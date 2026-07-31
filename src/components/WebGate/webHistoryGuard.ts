const GUARD_KEY = "codetermWebHistoryGuard";

type GuardState = Record<string, unknown> & { [GUARD_KEY]?: "root" | "guard" };

export function installWebHistoryGuard(): () => void {
  const current = (window.history.state ?? {}) as GuardState;
  if (current[GUARD_KEY] !== "guard") {
    const url = `${window.location.pathname}${window.location.search}${window.location.hash}`;
    window.history.replaceState({ ...current, [GUARD_KEY]: "root" }, "", url);
    window.history.pushState({ ...current, [GUARD_KEY]: "guard" }, "", url);
  }

  const restore = (event: PopStateEvent) => {
    const state = (event.state ?? {}) as GuardState;
    if (state[GUARD_KEY] === "guard") return;
    const url = `${window.location.pathname}${window.location.search}${window.location.hash}`;
    window.history.pushState({ ...state, [GUARD_KEY]: "guard" }, "", url);
  };
  window.addEventListener("popstate", restore);
  return () => window.removeEventListener("popstate", restore);
}
