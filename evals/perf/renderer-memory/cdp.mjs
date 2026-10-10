// Minimal Chrome DevTools Protocol client for the dev WebView2 renderer.
import { writeFileSync, createWriteStream } from "node:fs";

export async function pageTarget(cdpPort, vitePort) {
  const response = await fetch(`http://127.0.0.1:${cdpPort}/json/list`);
  if (!response.ok) throw new Error(`CDP list failed: ${response.status}`);
  const targets = await response.json();
  const page = targets.find(
    (target) => target.type === "page" && target.url.startsWith(`http://127.0.0.1:${vitePort}`),
  );
  if (!page) throw new Error(`no page target for vite port ${vitePort} on CDP ${cdpPort}`);
  return page;
}

export async function connect(wsUrl) {
  const socket = new WebSocket(wsUrl);
  await new Promise((resolve, reject) => {
    socket.addEventListener("open", resolve, { once: true });
    socket.addEventListener("error", reject, { once: true });
  });
  let nextId = 1;
  const pending = new Map();
  const handlers = new Map();
  socket.addEventListener("message", (event) => {
    const message = JSON.parse(event.data);
    if (message.id != null) {
      const entry = pending.get(message.id);
      pending.delete(message.id);
      if (message.error) entry?.reject(new Error(`${entry.method}: ${message.error.message}`));
      else entry?.resolve(message.result);
      return;
    }
    for (const handler of handlers.get(message.method) ?? []) handler(message.params);
  });
  return {
    send(method, params = {}) {
      const id = nextId++;
      socket.send(JSON.stringify({ id, method, params }));
      return new Promise((resolve, reject) => pending.set(id, { resolve, reject, method }));
    },
    on(method, handler) {
      const set = handlers.get(method) ?? new Set();
      set.add(handler);
      handlers.set(method, set);
      return () => set.delete(handler);
    },
    close() {
      socket.close();
    },
  };
}

export async function evaluate(client, expression) {
  const result = await client.send("Runtime.evaluate", {
    expression,
    returnByValue: true,
    awaitPromise: true,
  });
  if (result.exceptionDetails) {
    throw new Error(`evaluate failed: ${result.exceptionDetails.exception?.description ?? result.exceptionDetails.text}`);
  }
  return result.result.value;
}

export async function takeHeapSnapshot(client, path) {
  const out = createWriteStream(path);
  const off = client.on("HeapProfiler.addHeapSnapshotChunk", ({ chunk }) => out.write(chunk));
  await client.send("HeapProfiler.enable");
  await client.send("HeapProfiler.collectGarbage");
  await client.send("HeapProfiler.takeHeapSnapshot", { reportProgress: false, captureNumericValue: false });
  off();
  await new Promise((resolve) => out.end(resolve));
  return path;
}

export function writeJson(path, value) {
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);
}
