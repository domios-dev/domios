#!/usr/bin/env node

import http from "node:http";

const server = http.createServer((request, response) => {
  const chunks = [];
  request.on("data", (chunk) => chunks.push(chunk));
  request.on("end", () => {
    try {
      const kind = request.url === "/configure" ? "configure" : request.url === "/install" ? "install" : null;
      if (!kind || request.method !== "POST") {
        response.writeHead(404).end();
        return;
      }
      const value = JSON.parse(Buffer.concat(chunks).toString("utf8"));
      process.stdout.write(`OBSERVED ${kind} ${Buffer.from(JSON.stringify(value)).toString("base64url")}\n`);
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
  process.on(signal, () => server.close(() => process.exit(0)));
}
