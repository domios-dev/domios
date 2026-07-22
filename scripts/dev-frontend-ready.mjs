#!/usr/bin/env node

const mode = process.argv[2] || "ready";
const cdpPort = process.env.CODETERM_CDP_PORT;
const vitePort = process.env.CODETERM_VITE_PORT;

if (mode !== "ready" && mode !== "snapshot") {
  console.error("usage: dev-frontend-ready.mjs ready|snapshot");
  process.exit(2);
}

function finish(ok, message) {
  if (message) console.error(message);
  process.exit(ok ? 0 : 1);
}

function withTimeout(promise, ms) {
  return Promise.race([
    promise,
    new Promise((resolve) => setTimeout(() => resolve(null), ms)),
  ]);
}

async function pageTarget() {
  if (!cdpPort) return null;
  const targets = await withTimeout(
    fetch(`http://127.0.0.1:${cdpPort}/json/list`).then((response) => response.json()),
    3000,
  );
  if (!Array.isArray(targets)) return null;
  if (mode === "ready" && vitePort) {
    const prefix = `http://127.0.0.1:${vitePort}/`;
    return targets.find((target) => target.type === "page" && String(target.url || "").startsWith(prefix)) ?? null;
  }
  return targets.find((target) => target.type === "page") ?? null;
}

function evaluate(target, expression) {
  return withTimeout(new Promise((resolve) => {
    const ws = new WebSocket(target.webSocketDebuggerUrl);
    let settled = false;
    const settle = (value) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      ws.close();
      resolve(value);
    };
    const timer = setTimeout(() => {
      settle(null);
    }, 3000);
    ws.onopen = () => ws.send(JSON.stringify({
      id: 1,
      method: "Runtime.evaluate",
      params: { expression, returnByValue: true },
    }));
    ws.onerror = () => {
      settle(null);
    };
    ws.onmessage = (event) => {
      let msg;
      try {
        msg = JSON.parse(event.data);
      } catch {
        settle(null);
        return;
      }
      if (msg.id !== 1) return;
      settle(msg.result?.result?.value ?? null);
    };
  }), 3500);
}

async function ready() {
  const target = await pageTarget();
  if (!target?.webSocketDebuggerUrl) return finish(false);
  const ok = await evaluate(
    target,
    "document.readyState === 'complete' && !!document.querySelector('#root > *')",
  );
  finish(ok === true);
}

async function snapshot() {
  const target = await pageTarget();
  if (!target?.webSocketDebuggerUrl) return finish(true, "frontend: no WebView2 page target");
  const state = await evaluate(
    target,
    "JSON.stringify({href:location.href,ready:document.readyState,rootChildren:document.querySelector('#root')?.children.length??-1,rootText:(document.querySelector('#root')?.textContent||'').slice(0,160),bodyText:document.body.innerText.slice(0,160)})",
  );
  finish(true, state ? `frontend: ${state}` : "frontend: WebView2 target did not answer Runtime.evaluate");
}

if (mode === "snapshot") {
  snapshot().catch((error) => finish(true, `frontend: snapshot failed: ${error?.message || error}`));
} else {
  ready().catch(() => finish(false));
}
