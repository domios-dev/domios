(() => {
  const storageKey = "codeterm.frontend.boot";
  let context;
  try { context = JSON.parse(sessionStorage.getItem(storageKey) || "null"); } catch { return; }
  if (!context || !context.ticket || !context.apiOrigin || !context.token) return;
  const api = new URL(context.apiOrigin);
  if (!["http:", "https:"].includes(api.protocol)) return;
  const origin = api.origin;
  const ticket = context.ticket;
  if (!location.pathname.includes(`/frontend/${ticket.releaseId}/`)) return;
  const headers = { Authorization: `Bearer ${context.token}`, "Content-Type": "application/json" };
  let rendered = false;
  let failing = false;
  const request = async (path, body) => {
    const response = await fetch(origin + path, { method: "POST", headers, body: JSON.stringify(body) });
    if (!response.ok) throw new Error("Frontend observer request failed");
    return response.json();
  };
  const fail = async () => {
    if (failing) return;
    failing = true;
    const attempted = [...new Set([...(context.attempted || []), ticket.releaseId])];
    if (attempted.length >= 3) return;
    try {
      let status;
      try { status = await request("/api/frontend/rollback", { releaseId: ticket.releaseId, generation: ticket.generation }); }
      catch {
        const response = await fetch(origin + "/api/frontend/status", { headers });
        if (!response.ok) return;
        status = await response.json();
      }
      const previous = status.active;
      if (previous && previous.minApiVersion <= status.apiVersion && status.apiVersion <= previous.maxApiVersion && !attempted.includes(previous.releaseId)) {
        const response = await request("/api/frontend/activate", { releaseId: previous.releaseId, clients: [context.clientId] });
        const next = { releaseId: response.releaseId, generation: response.generation, token: response.claims[context.clientId] };
        sessionStorage.setItem(storageKey, JSON.stringify({ ...context, ticket: next, attempted }));
        location.replace(`/frontend/${encodeURIComponent(next.releaseId)}/index.html?generation=${next.generation}`);
        return;
      }
      const fallback = context.bundled;
      if (fallback && fallback.minApiVersion <= status.apiVersion && status.apiVersion <= fallback.maxApiVersion && !attempted.includes("bundled")) {
        const target = new URL(fallback.index, location.origin);
        if (target.origin !== location.origin) return;
        sessionStorage.setItem(storageKey, JSON.stringify({ ...context, ticket: null, attempted: [...attempted, "bundled"] }));
        location.replace(target.href);
      }
    } catch {}
  };
  addEventListener("error", () => { void fail(); }, true);
  addEventListener("unhandledrejection", () => { void fail(); });
  addEventListener("codeterm:frontend-root-rendered", event => {
    if (failing || rendered || !event.detail || event.detail.releaseId !== ticket.releaseId || event.detail.generation !== ticket.generation) return;
    rendered = true;
    void request("/api/frontend/healthy", { ...ticket, clientId: context.clientId }).catch(() => { void fail(); });
  });
  AbortSignal.timeout(30000).addEventListener("abort", () => { if (!rendered) void fail(); }, { once: true });
})();
