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
import { generateKeyPairSync, sign as edSign, randomUUID, type KeyObject } from "node:crypto";
import { RELAY_PROTOCOL_VERSION } from "../../../src/lib/types";

export type StubMode = "happy" | "wedged" | "liar";

export class MeshStub {
  private server: http.Server | null = null;
  port = 0;
  mode: StubMode = "happy";
  /** The stub's own stable identity. Manual pairing remaps the daemon's
   *  `manual-<uuid>` placeholder to this UUID, so it must be a real UUID that
   *  `crown`/`Uuid::parse_str` accepts — never the placeholder. */
  instanceId = randomUUID();
  readonly instanceName = "pw-mesh-stub";
  /** A real peer reports a protected General AND ordinary workspaces; serving only
   *  the protected one describes a peer shape the product cannot produce. */
  readonly generalWorkspaceId = `${this.instanceName}-general-ws`;
  readonly generalWorkspaceName = `${this.instanceName}-general`;
  readonly workspaceId = `${this.instanceName}-ws`;
  readonly workspaceName = `${this.instanceName}-workspace`;
  /** Real Ed25519 identity so the daemon accepts our signed announce (crown auth)
   *  and, via it, bumps our relay_protocol_version — required for versioned
   *  relay-stream / relay-to. Random-key stubs can only pair + answer pings. */
  readonly publicKey: string;
  private readonly signingKey: KeyObject;
  /** method+path log so tests can assert what the daemon actually called. */
  readonly requests: string[] = [];
  private serviceCount: number;
  /** Daemon base for the pair-accept callback. */
  private daemonBase: string;
  private pairAcceptCompleted = false;
  private pairAcceptError: Error | null = null;
  private pairAcceptWaiters = new Set<{ resolve: () => void; reject: (error: Error) => void }>();

  /** When set, the stub also answers the web-mirror's direct API calls
   * (gate validation, mux state, peers …) gated by this bearer token —
   * lets 12-web exercise real peer SWITCHING against a scripted host. */
  webToken: string | null = null;
  /** Config payload for /api/config — seed with a REAL daemon config so
   * the SPA's boot path doesn't choke on missing fields. */
  configPayload: unknown = {};
  muxDelayMs = 0;

  /** Remote-tab projection (D2): the single local tab this stub advertises so
   *  the dev daemon can project `mesh:<peer>/<remoteTabId>` (remote-tab-render). */
  remoteTabId = "shared";
  remotePtyId = "shared-pty";
  /** Transcript-authority identity echoed by the chat snapshot + live events so
   *  the projected tab (agent_session_id=sourceSessionId) adopts them. */
  sourceSessionId = "pw-stub-session";
  sessionBindingNonce = "pw-stub-nonce";
  /** G4 switch: when set, the chat events stream fails (500) and every open chat
   *  stream is closed, while the PTY output stream stays live and usable. */
  chatFail = false;

  private readonly outputClients = new Set<http.ServerResponse>();
  private readonly chatClients = new Set<http.ServerResponse>();
  /** Append-only PTY frame log; replayed (offset > since) on every (re)connect
   *  so cursor-resume delivers exactly what a live xterm missed. */
  private readonly ptyLog: Array<{ output: string; offset: number }> = [];
  private readonly chatLog: Array<{ frame: string; seq: number }> = [];
  private readonly outputBoot = "pw-out-boot";
  private readonly chatBoot = "pw-chat-boot";
  private ptyOffset = 0;
  private chatSeq = 0;

  constructor(opts: { services?: number; daemonBase?: string; webToken?: string } = {}) {
    this.serviceCount = opts.services ?? 3;
    this.daemonBase = opts.daemonBase ?? "http://127.0.0.1:7686";
    this.webToken = opts.webToken ?? null;
    const { publicKey, privateKey } = generateKeyPairSync("ed25519");
    this.signingKey = privateKey;
    // Raw 32-byte Ed25519 public key (last 32 bytes of the SPKI DER), base64 —
    // exactly what crown::verify_signed_request expects for this instance_id.
    this.publicKey = publicKey.export({ type: "spki", format: "der" }).subarray(-32).toString("base64");
  }

  /** crown signed-request token: base64(json).base64(ed25519_sig), Bearer-carried. */
  private signToken(): string {
    const payloadB64 = Buffer.from(
      JSON.stringify({
        from_instance_id: this.instanceId,
        timestamp: Math.floor(Date.now() / 1000),
        nonce: randomUUID(),
      }),
    ).toString("base64");
    const sig = edSign(null, Buffer.from(payloadB64), this.signingKey).toString("base64");
    return `${payloadB64}.${sig}`;
  }

  /** Announce our relay protocol version to the (already-paired) daemon so it
   *  will open versioned relay-stream / relay-to connections to us. Empty
   *  addresses + online keep the daemon's existing 127.0.0.1 route intact. */
  async announce(relayProtocolVersion = RELAY_PROTOCOL_VERSION): Promise<void> {
    const res = await fetch(`${this.daemonBase}/api/mesh/announce`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${this.signToken()}`,
      },
      body: JSON.stringify({
        instance_id: this.instanceId,
        instance_name: this.instanceName,
        addresses: [],
        api_port: this.port,
        online: true,
        web_ui: null,
        relay_protocol_version: relayProtocolVersion,
      }),
      signal: AbortSignal.timeout(5_000),
    });
    if (!res.ok) {
      throw new Error(`mesh-stub announce failed: HTTP ${res.status} ${await res.text()}`);
    }
  }

  private workspaceShapes(): Array<{ id: string; name: string; isProtected: boolean }> {
    return [
      { id: this.generalWorkspaceId, name: this.generalWorkspaceName, isProtected: true },
      { id: this.workspaceId, name: this.workspaceName, isProtected: false },
    ];
  }

  /** Workspaces as the peer's mux state reports them. */
  muxWorkspaces(): Array<Record<string, unknown>> {
    return this.workspaceShapes().map((workspace) => ({
      id: workspace.id,
      name: workspace.name,
      tabs: [],
      active_tab_id: null,
      cwd: null,
      protected: workspace.isProtected,
    }));
  }

  /** The same set in the flatter `/api/workspaces` shape. */
  listWorkspaces(): Array<Record<string, unknown>> {
    return this.workspaceShapes().map((workspace) => ({
      id: workspace.id,
      name: workspace.name,
      is_active: workspace.id === this.workspaceId,
      tab_count: 0,
      cwd: null,
      protected: workspace.isProtected,
      tab_ids: [],
    }));
  }

  /** The projected remote tab as the peer's `/api/tabs` would report it. */
  tabPayload(): Record<string, unknown> {
    return {
      id: this.remoteTabId,
      tab_id: this.remoteTabId,
      title: "PW Remote Shared",
      workspace_id: this.generalWorkspaceId,
      pty_id: this.remotePtyId,
      agent_type: "claude",
      agent_session_id: this.sourceSessionId,
      session_binding_nonce: this.sessionBindingNonce,
      backing: { kind: "pty" },
      backing_kind: "pty",
      chat_view: true,
      status: "working",
      role: "Worker",
      hidden: false,
      show_in_tabbar: true,
    };
  }

  private chatStartSnapshot(): Record<string, unknown> {
    return {
      text: "",
      offset: 1,
      start_offset: 0,
      pending: false,
      source_session_id: this.sourceSessionId,
      session_binding_nonce: this.sessionBindingNonce,
      subscription_id: "pw-sub-1",
      messages: [
        { id: "pw-chat-snapshot", type: "assistant", content: "PW-CHAT-SNAPSHOT", timestamp: "1" },
      ],
    };
  }

  /** Emit a PTY output frame to every open output stream and record it for
   *  cursor-resumed replay. Returns the new byte offset. */
  emitPty(text: string): number {
    this.ptyOffset += text.length;
    const offset = this.ptyOffset;
    this.ptyLog.push({ output: text, offset });
    const line = `data: ${JSON.stringify({
      tab_id: this.remoteTabId,
      output: text,
      offset,
      boot: this.outputBoot,
    })}\n\n`;
    for (const res of this.outputClients) res.write(line);
    return offset;
  }

  /** Emit a live tab-chat-msg event (authoritative for the projected tab). */
  emitChat(content: string, id: string): number {
    this.chatSeq += 1;
    const seq = this.chatSeq;
    const line = `data: ${JSON.stringify({
      event_type: `tab-chat-msg-${this.remoteTabId}`,
      boot: this.chatBoot,
      seq,
      data: {
        tab_id: this.remoteTabId,
        source_session_id: this.sourceSessionId,
        session_binding_nonce: this.sessionBindingNonce,
        messages: [{ id, type: "assistant", content, timestamp: String(seq) }],
      },
    })}\n\n`;
    this.chatLog.push({ frame: line, seq });
    for (const res of this.chatClients) res.write(line);
    return seq;
  }

  /** G4: kill the chat channel — close open chat streams and make new ones 500,
   *  leaving the PTY output channel untouched. */
  failChat(): void {
    this.chatFail = true;
    for (const res of this.chatClients) {
      try {
        res.end();
      } catch {
        /* already closed */
      }
    }
    this.chatClients.clear();
  }

  private parseOutputSince(since: string | null): number {
    if (!since) return 0;
    try {
      const json = Buffer.from(since.replace(/-/g, "+").replace(/_/g, "/"), "base64").toString("utf8");
      const parsed = JSON.parse(json) as { boot?: string; tabs?: Record<string, number> };
      if (parsed.boot && parsed.boot !== this.outputBoot) return 0;
      return parsed.tabs?.[this.remoteTabId] ?? 0;
    } catch {
      return 0;
    }
  }

  private parseChatSince(since: string | null): number {
    if (!since) return 0;
    const [boot, seq] = since.split(":");
    if (boot !== this.chatBoot) return 0;
    const parsed = Number(seq);
    return Number.isFinite(parsed) ? parsed : 0;
  }

  private serveOutputStream(req: http.IncomingMessage, res: http.ServerResponse): void {
    const url = new URL(req.url ?? "", "http://127.0.0.1");
    const sinceOffset = this.parseOutputSince(url.searchParams.get("since"));
    res.writeHead(200, {
      "Content-Type": "text/event-stream",
      "Cache-Control": "no-cache",
      "Access-Control-Allow-Origin": "*",
    });
    res.write(": pw output stream\n\n");
    for (const frame of this.ptyLog) {
      if (frame.offset > sinceOffset) {
        res.write(
          `data: ${JSON.stringify({
            tab_id: this.remoteTabId,
            output: frame.output,
            offset: frame.offset,
            boot: this.outputBoot,
          })}\n\n`,
        );
      }
    }
    this.outputClients.add(res);
    res.on("close", () => this.outputClients.delete(res));
  }

  private serveEventsStream(req: http.IncomingMessage, res: http.ServerResponse): void {
    if (this.chatFail) {
      res.writeHead(500, {
        "Content-Type": "application/json",
        "Access-Control-Allow-Origin": "*",
      });
      res.end(JSON.stringify({ error: "pw chat stream failing" }));
      return;
    }
    const url = new URL(req.url ?? "", "http://127.0.0.1");
    const sinceSeq = this.parseChatSince(url.searchParams.get("since"));
    res.writeHead(200, {
      "Content-Type": "text/event-stream",
      "Cache-Control": "no-cache",
      "Access-Control-Allow-Origin": "*",
    });
    res.write(": pw events stream\n\n");
    for (const entry of this.chatLog) {
      if (entry.seq > sinceSeq) res.write(entry.frame);
    }
    this.chatClients.add(res);
    res.on("close", () => this.chatClients.delete(res));
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

  waitForPairAccept(timeoutMs = 10_000): Promise<void> {
    if (this.pairAcceptCompleted) {
      return this.pairAcceptError ? Promise.reject(this.pairAcceptError) : Promise.resolve();
    }
    return new Promise((resolve, reject) => {
      let timer: ReturnType<typeof setTimeout>;
      const waiter = {
        resolve: () => {
          clearTimeout(timer);
          resolve();
        },
        reject: (error: Error) => {
          clearTimeout(timer);
          reject(error);
        },
      };
      timer = setTimeout(() => {
        this.pairAcceptWaiters.delete(waiter);
        reject(new Error(`mesh stub pair-accept callback did not finish within ${timeoutMs}ms`));
      }, timeoutMs);
      this.pairAcceptWaiters.add(waiter);
    });
  }

  private completePairAccept(error: Error | null): void {
    this.pairAcceptCompleted = true;
    this.pairAcceptError = error;
    for (const waiter of this.pairAcceptWaiters) {
      if (error) waiter.reject(error);
      else waiter.resolve();
    }
    this.pairAcceptWaiters.clear();
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

    // Cursor'd SSE streams the dev daemon opens directly on the peer via
    // `open_relay_stream` (GET <base>/api/{output,events}/stream?tab_id&since).
    if (path === "/api/output/stream") {
      req.resume();
      return this.serveOutputStream(req, res);
    }
    if (path === "/api/events/stream" && !this.webToken) {
      req.resume();
      return this.serveEventsStream(req, res);
    }

    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      const json = (status: number, value: unknown) => {
        res.writeHead(status, {
          "Content-Type": "application/json",
          "Access-Control-Allow-Origin": "*",
          "Access-Control-Allow-Headers": "*",
          "Access-Control-Allow-Methods": "*",
        });
        res.end(JSON.stringify(value));
      };
      if (req.method === "OPTIONS") return json(204, null);

      if (path === "/api/mesh/web-token" && this.webToken) {
        const auth = req.headers.authorization ?? "";
        if (!auth.startsWith("Bearer ")) return json(401, { error: "missing mesh auth" });
        return json(200, { ok: true, token: this.webToken });
      }

      if (this.webToken && this.handleWebApi(req, res, path, json)) return;

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
          res.once("finish", () => void this.sendAccept(request_id));
          return json(200, { ok: true, instance_id: this.instanceId });
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
            if (cmd.path) {
              this.requests.push(`${req.method} ${path}:${cmd.path}`);
            }
            if (cmd.method === "GET" && cmd.path === "/api/services") {
              return json(200, { ok: true, status: 200, data: this.services(), error: null });
            }
            if (cmd.method === "GET" && cmd.path === "/api/mux/state") {
              const sendMux = () =>
                json(200, {
                  ok: true,
                  status: 200,
                  data: {
                    workspaces: this.muxWorkspaces(),
                    tabs: {},
                    active_workspace_id: this.workspaceId,
                    focused_tab_id: null,
                  },
                  error: null,
                });
              if (this.muxDelayMs > 0) {
                setTimeout(sendMux, this.muxDelayMs);
                return;
              }
              return sendMux();
            }
            const bare = (cmd.path ?? "").split("?")[0];
            if (cmd.method === "GET" && bare === "/api/tabs") {
              return json(200, { ok: true, status: 200, data: [this.tabPayload()], error: null });
            }
            if (cmd.method === "GET" && bare === "/api/tabs/inspect") {
              return json(200, {
                ok: true,
                status: 200,
                data: [{ id: this.remoteTabId, tab_id: this.remoteTabId, pty_id: this.remotePtyId }],
                error: null,
              });
            }
            if (cmd.method === "POST" && bare === `/api/tabs/${this.remoteTabId}/chat-raw/start`) {
              return json(200, { ok: true, status: 200, data: this.chatStartSnapshot(), error: null });
            }
            // List endpoints a real peer returns as arrays — never null, which
            // would crash the composer (`delayedMessages.filter`) on tab select.
            if (
              cmd.method === "GET" &&
              (bare.endsWith("/delayed-messages") || bare.endsWith("/shell-commands"))
            ) {
              return json(200, { ok: true, status: 200, data: [], error: null });
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

  /** Minimal web-mirror API: enough for the SPA to pass the token gate
   * and render against this stub as a switched-to peer. Returns true when
   * the path was handled. */
  private handleWebApi(
    req: http.IncomingMessage,
    res: http.ServerResponse,
    path: string,
    json: (status: number, value: unknown) => void,
  ): boolean {
    const WEB_PATHS = new Set([
      "/api/workspaces",
      "/api/mux/state",
      "/api/config",
      "/api/mesh/peers",
      "/api/mesh/status",
      "/api/agents/providers",
      "/api/services",
      "/api/activity",
      "/api/health",
      "/api/events/stream",
    ]);
    if (!WEB_PATHS.has(path)) return false;

    const auth = req.headers.authorization ?? "";
    if (path !== "/api/health" && auth !== `Bearer ${this.webToken}`) {
      json(401, { error: "invalid token" });
      return true;
    }
    if (path === "/api/events/stream") {
      // instead of reconnect-spamming a JSON 200.
      res.writeHead(200, {
        "Content-Type": "text/event-stream",
        "Cache-Control": "no-cache",
        "Access-Control-Allow-Origin": "*",
        "Access-Control-Allow-Headers": "*",
      });
      res.write(": stub stream\n\n");
      return true;
    }
    switch (path) {
      case "/api/health":
        json(200, { status: "ok", build: "stub", version: "0.0.0" });
        break;
      case "/api/workspaces":
        json(200, this.listWorkspaces());
        break;
      case "/api/mux/state":
        json(200, {
          workspaces: this.muxWorkspaces(),
          tabs: {},
          active_workspace_id: this.workspaceId,
          focused_tab_id: null,
        });
        break;
      case "/api/config":
        json(200, this.configPayload);
        break;
      case "/api/mesh/peers":
        json(200, { paired: [], discovered: [], incoming_requests: [] });
        break;
      case "/api/mesh/status":
        json(200, {
          enabled: true,
          instance_name: this.instanceName,
          local_ip: "127.0.0.1",
          tailscale_ip: null,
        });
        break;
      default:
        json(200, []);
    }
    return true;
  }

  private async sendAccept(requestId: string): Promise<void> {
    try {
      const response = await fetch(`${this.daemonBase}/api/mesh/pair/accept`, {
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
      if (!response.ok) {
        throw new Error(`HTTP ${response.status}`);
      }
      this.completePairAccept(null);
    } catch (e) {
      const error = e instanceof Error ? e : new Error(String(e));
      this.completePairAccept(error);
      console.warn(`[mesh-stub] pair-accept callback failed: ${error.message}`);
    }
  }
}
