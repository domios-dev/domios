#!/usr/bin/env node

import http from "node:http";

const observations = [];
const waiters = [];

function matches(observation, kind, token) {
  return observation.kind === kind
    && (observation.value?.token === token || observation.value?.task === token);
}

function deliver(observation) {
  observations.push(observation);
  for (let index = waiters.length - 1; index >= 0; index -= 1) {
    const waiter = waiters[index];
    if (!matches(observation, waiter.kind, waiter.token)) continue;
    waiters.splice(index, 1);
    waiter.response.writeHead(200, { "content-type": "application/json" });
    waiter.response.end(JSON.stringify(observation.value));
  }
}

const server = http.createServer((request, response) => {
  const url = new URL(request.url || "/", "http://127.0.0.1");
  if (request.method === "GET" && url.pathname === "/wait") {
    const kind = url.searchParams.get("kind") || "";
    const token = url.searchParams.get("token") || "";
    const observed = observations.find((item) => matches(item, kind, token));
    if (observed) {
      response.writeHead(200, { "content-type": "application/json" });
      response.end(JSON.stringify(observed.value));
      return;
    }
    const waiter = { kind, token, response };
    waiters.push(waiter);
    response.on("close", () => {
      const index = waiters.indexOf(waiter);
      if (index >= 0) waiters.splice(index, 1);
    });
    return;
  }
  const chunks = [];
  request.on("data", (chunk) => chunks.push(chunk));
  request.on("end", () => {
    try {
      const kind = url.pathname === "/configure" ? "configure" : url.pathname === "/install" ? "install" : null;
      if (!kind || request.method !== "POST") {
        response.writeHead(404).end();
        return;
      }
      const value = JSON.parse(Buffer.concat(chunks).toString("utf8"));
      deliver({ kind, value });
      response.writeHead(204).end();
    } catch (error) {
      response.writeHead(400).end(String(error));
    }
  });
});

server.listen(0, "127.0.0.1", () => {
  const address = server.address();
  process.stdout.write(`READY http://127.0.0.1:${address.port}\n`);
});

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => {
    for (const waiter of waiters.splice(0)) waiter.response.destroy();
    server.close(() => process.exit(0));
  });
}
