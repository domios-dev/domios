/**
 * Mesh peer simulator (testplan §4.2) — an in-process HTTP stub speaking
 * just enough of the mesh surface for the dev daemon to pair with it and
 * track its liveness. TS instead of the suggested Rust binary: the only
 * consumer is this harness, in-process needs no build step or child
 * process management, and mode switches are instant.
 *
 * Modes:
 * - happy  — answers everything, advertises N services
 * - wedged — accepts TCP but never responds (#130 scenario)
 * - liar   — alive, but service/relay endpoints error
 */
import http from "node:http";
import { randomBytes } from "node:crypto";

export type StubMode = "happy" | "wedged" | "liar";

export class MeshStub {
  private server: http.Server | null = null;
  port = 0;
  mode: StubMode = "happy";
  /** Set after the daemon assigns a manual-peer id (add-manual response). */
  instanceId = "pw-mesh-stub-unset";
  readonly instanceName = "pw-mesh-stub";
  readonly publicKey = randomBytes(32).toString("base64");
  /** method+path log so tests can assert what the daemon actually called. */
  readonly requests: string[] = [];
  private serviceCount: number;
  /** Daemon base for the pair-accept callback. */
  private daemonBase: string;

  constructor(opts: { services?: number; daemonBase?: string } = {}) {
    this.serviceCount = opts.services ?? 3;
    this.daemonBase = opts.daemonBase ?? "http://127.0.0.1:7686";
  }

  start(mode: StubMode = "happy"): Promise<number> {
    this.mode = mode;
    this.server = http.createServer((req, res) => this.handle(req, res));
    // Keep wedged sockets from being reaped by default timeouts.
    this.server.requestTimeout = 0;
    this.server.headersTimeout = 0;
    return new Promise((resolve, reject) => {
      this.server!.once("error", reject);
      this.server!.listen(0, "127.0.0.1", () => {
        const addr = this.server!.address();
        if (addr && typeof addr === "object") {
          this.port = addr.port;
          resolve(this.port);
        } else {
          reject(new Error("stub failed to bind"));
        }
      });
    });
  }

  stop(): Promise<void> {
    return new Promise((resolve) => {
      if (!this.server) return resolve();
      this.server.closeAllConnections();
      this.server.close(() => resolve());
      this.server = null;
    });
  }

  services(): unknown[] {
    return Array.from({ length: this.serviceCount }, (_, i) => ({
      id: `pw-stub-svc-${i}`,
      name: `pw-stub-service-${i}`,
      kind: "daemon",
      health_mode: "http",
      health_interval: 60,
      health: { status: "healthy" },
      push_enabled: false,
      registered_at: 0,
      updated_at: 0,
      enabled: true,
    }));
  }

  private handle(req: http.IncomingMessage, res: http.ServerResponse): void {
    const path = (req.url ?? "").split("?")[0];
    this.requests.push(`${req.method} ${path}`);

    if (this.mode === "wedged") {
      // Accept the connection, read the request, never answer.
      req.resume();
      return;
    }

    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      const json = (status: number, value: unknown) => {
        res.writeHead(status, { "Content-Type": "application/json" });
        res.end(JSON.stringify(value));
      };

      switch (path) {
        case "/api/mesh/ping":
          return json(200, { ok: true });

        case "/api/mesh/pair/request": {
          // Approve asynchronously, like a real peer would.
          let request_id = "";
          try {
            request_id = (JSON.parse(body) as { request_id: string }).request_id;
          } catch {
            return json(400, { error: "bad pairing request body" });
          }
          setTimeout(() => void this.sendAccept(request_id), 150);
          return json(200, { ok: true });
        }

        case "/api/mesh/sync":
          return json(200, { peers: [] });

        case "/api/services":
          return this.mode === "liar"
            ? json(500, { error: "liar peer refuses services" })
            : json(200, this.services());

        case "/api/mesh/relay": {
          if (this.mode === "liar") {
            return json(200, { ok: false, status: 500, data: null, error: "liar peer" });
          }
          try {
            const cmd = JSON.parse(body) as { method?: string; path?: string };
            if (cmd.method === "GET" && cmd.path === "/api/services") {
              return json(200, { ok: true, status: 200, data: this.services(), error: null });
            }
          } catch {
            /* fall through */
          }
          return json(200, { ok: true, status: 200, data: null, error: null });
        }

        default:
          // announce, introduce, gossip extras — acknowledge politely.
          return json(200, { ok: true });
      }
    });
  }

  private async sendAccept(requestId: string): Promise<void> {
    try {
      await fetch(`${this.daemonBase}/api/mesh/pair/accept`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          request_id: requestId,
          from_instance_id: this.instanceId,
          from_instance_name: this.instanceName,
          public_key: this.publicKey,
        }),
        signal: AbortSignal.timeout(5_000),
      });
    } catch (e) {
      // Surfaced via the test's pairing-poll timeout; log for diagnosis.
      console.warn(`[mesh-stub] pair-accept callback failed: ${e}`);
    }
  }
}
