// Minimal CDP client for the live DEV webview (never prod): one page, request/response and events.
import { devConnection } from "../../evals/_lib/connection.mjs";

const DEFAULT_PORT = Number(process.env.CODETERM_DEV_CDP_PORT || 9223);

/** `urlMatch` picks one page of a multi-page browser, such as a Domios Browser tab serving a lane frontend. */
export async function connectPage(port = DEFAULT_PORT, urlMatch = process.env.CODETERM_DEV_CDP_URL) {
  const pages = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
  const page = pages.find((entry) => entry.type === "page" && (!urlMatch || entry.url.includes(urlMatch)))
    ?? (urlMatch ? undefined : pages.find((entry) => entry.type === "page"));
  if (!page) throw new Error(`No DEV page${urlMatch ? ` matching ${urlMatch}` : ""} on CDP port ${port}`);
  const socket = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    socket.addEventListener("open", resolve, { once: true });
    socket.addEventListener("error", reject, { once: true });
  });
  let nextId = 0;
  const pending = new Map();
  const listeners = new Map();
  socket.addEventListener("message", (event) => {
    const message = JSON.parse(event.data);
    if (message.id && pending.has(message.id)) {
      const { resolve, reject } = pending.get(message.id);
      pending.delete(message.id);
      if (message.error) reject(new Error(`${message.error.message} (${message.error.code})`));
      else resolve(message.result);
    } else if (message.method) {
      for (const listener of listeners.get(message.method) ?? []) listener(message.params);
    }
  });
  const call = (method, params = {}) => new Promise((resolve, reject) => {
    const id = ++nextId;
    pending.set(id, { resolve, reject });
    socket.send(JSON.stringify({ id, method, params }));
  });
  const evaluate = async (expression) => {
    const result = await call("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
    if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text);
    return result.result?.value;
  };
  const on = (method, listener) => listeners.set(method, [...(listeners.get(method) ?? []), listener]);
  return { call, evaluate, on, close: () => socket.close() };
}

export function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/** Reloads with `source` installed before any page script; the session must stay open while it is needed. */
export async function reloadWithScript(page, source, readyExpression) {
  await page.call("Page.enable");
  await page.call("Page.addScriptToEvaluateOnNewDocument", { source });
  await page.call("Page.reload", {});
  for (let attempt = 0; attempt < 60; attempt++) {
    await sleep(500);
    try {
      if (await page.evaluate(readyExpression)) return;
    } catch {
      // The document is still navigating.
    }
  }
  throw new Error("DEV page did not become ready after reload");
}

export function devApiToken() {
  return devConnection().token;
}

export const APP_READY ="document.querySelectorAll('[data-testid=workspace-detail]').length > 0";
