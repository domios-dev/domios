import { execFile } from "node:child_process";
import { Agent, request as httpRequest } from "node:http";
import { promisify } from "node:util";
import { fileURLToPath } from "node:url";
import { bashInterpreter } from "../../../scripts/bash-interpreter.mjs";

const runner = fileURLToPath(new URL("./run.sh", import.meta.url));
const run = promisify(execFile);
const http = new Agent({ keepAlive: true });

export function closeConnections() { http.destroy(); }

export function currentConnection() {
  return {
    instance: process.env.CODETERM_INSTANCE,
    origin: process.env.CODETERM_API_URL,
    token: process.env.CODETERM_API_TOKEN,
  };
}

async function benchCommand(connection, args) {
  const port = new URL(connection.origin).port;
  try {
    const { stdout } = await run(bashInterpreter(), [runner, ...args], {
      env: { ...process.env, CODETERM_INSTANCE: connection.instance, CODETERM_API_PORT: port },
      timeout: 45_000,
      maxBuffer: 1024 * 1024,
      windowsHide: true,
    });
    return JSON.parse(stdout);
  } catch (error) {
    const detail = args[0] === "--dc" ? (error.stdout || error.stderr || "").trim().slice(0, 800) : "";
    throw new Error(`DEV ${args.slice(0, 3).join(" ")} failed (exit ${error.code ?? "unknown"})${detail ? `: ${detail}` : ""}`);
  }
}

export async function dc(connection, args) {
  return (await benchCommand(connection, ["--dc", ...args, "--json"])).result;
}

export function gatewayConnection(instance, port) {
  return benchCommand({ instance, origin: `http://127.0.0.1:${port}` }, ["--connection"]);
}

export function checkedConnection(connection) {
  const url = new URL(connection.origin);
  if (!connection.token || !/^[a-z0-9][a-z0-9-]*$/.test(connection.instance) ||
      url.protocol !== "http:" || url.hostname !== "127.0.0.1" ||
      connection.origin !== url.origin || Number(url.port) < 1024 || ["7685", "7686"].includes(url.port)) {
    throw new Error("Expected a private loopback connection from the named DEV wrapper");
  }
  return Number(url.port);
}

function request(connection, route, { client, method, body, signal }) {
  if (!route.startsWith("/api/")) throw new Error("Expected a local route");
  const payload = body === undefined ? undefined : Buffer.from(JSON.stringify(body));
  return new Promise((resolve, reject) => {
    const req = httpRequest(connection.origin + route, {
      method, signal, agent: http,
      headers: {
        Authorization: `Bearer ${connection.token}`,
        "Content-Type": "application/json",
        "X-CodeTerm-Client-Id": client,
        ...(payload && { "Content-Length": payload.length }),
      },
    }, resolve);
    req.on("error", reject);
    req.end(payload);
  });
}

export async function api(connection, route, {
  client = "server-chat-load", method = "GET", body, signal = AbortSignal.timeout(15_000),
} = {}) {
  const started = performance.now();
  const response = await request(connection, route, { client, method, body, signal });
  const chunks = [];
  for await (const chunk of response) chunks.push(chunk);
  const bytes = Buffer.concat(chunks);
  if (response.statusCode < 200 || response.statusCode >= 300) {
    throw new Error(`${method} ${route}: HTTP ${response.statusCode}`);
  }
  return {
    value: JSON.parse(bytes.toString()),
    bytes: bytes.byteLength,
    wallMs: performance.now() - started,
  };
}

export async function events(connection, route, client, consume, signal) {
  const close = new AbortController();
  const deadline = AbortSignal.any([signal, close.signal]);
  const started = performance.now();
  const response = await request(connection, route, { client, signal: deadline });
  if (response.statusCode !== 200 || !response.headers["content-type"]?.startsWith("text/event-stream")) {
    response.destroy();
    throw new Error(`${client} event stream: HTTP ${response.statusCode}`);
  }
  const ready = Promise.withResolvers();
  const stats = { wireBytes: 0, dataFrames: 0, overflows: 0, attachMs: null };
  const decoder = new TextDecoder();
  let pending = "";
  const task = (async () => {
    for await (const value of response) {
      stats.wireBytes += value.byteLength;
      pending += decoder.decode(value, { stream: true });
      let separator;
      while ((separator = pending.indexOf("\n\n")) !== -1) {
        const frame = pending.slice(0, separator);
        pending = pending.slice(separator + 2);
        if (!frame.startsWith("data:")) continue;
        const event = JSON.parse(frame.slice(5));
        stats.dataFrames++;
        if (event.event_type === "overflow") stats.overflows++;
        if (stats.attachMs === null) {
          if (event.event_type !== "overflow" || event.data?.reason !== "boot-changed") {
            throw new Error(`${client} received no boot fence`);
          }
          stats.attachMs = performance.now() - started;
          ready.resolve();
        } else {
          if (event.event_type === "overflow") throw new Error(`${client} lost events: ${event.data?.reason}`);
          consume(event);
        }
      }
      if (pending.length > 4 * 1024 * 1024) throw new Error(`${client} frame exceeds measurement bound`);
    }
    throw new Error(`${client} stream ended before close`);
  })();
  const stream = { stats, task, async close() {
    close.abort();
    response.destroy();
    await task.catch(() => {});
  } };
  task.catch(error => ready.reject(error));
  try {
    await ready.promise;
    return stream;
  } catch (error) {
    await stream.close();
    throw error;
  }
}

export function viewerRoutes(tab, peer) {
  const path = "/api/events/stream";
  const snapshot = `/api/tabs/${encodeURIComponent(tab)}/chat-raw/start`;
  const body = { subscriptionId: `server-load-viewer-${tab}` };
  // A different boot cursor gives the subscriber an immediate protocol fence.
  const since = "bench-attach:0";
  if (!peer) {
    return {
      stream: `${path}?${new URLSearchParams({ tab_id: tab, scope: "tab", since })}`,
      snapshot,
      body,
    };
  }
  return {
    stream: `/api/mesh/relay-stream?${new URLSearchParams({ peer, path, tab: `mesh:${peer}/${tab}`, since })}`,
    snapshot: "/api/mesh/relay-to",
    body: { target_instance_id: peer, method: "POST", path: snapshot, body },
  };
}
