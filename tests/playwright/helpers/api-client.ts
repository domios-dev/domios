/**
 * Typed wrapper over the selected Playwright target's REST API.
 * Token comes from that target's dev data directory.
 */
import { existsSync, readFileSync, readdirSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { currentDevInstance, isDevTarget, isNamedTarget, type DevInstance } from "./dev-instance";

export interface Tab {
  id: string;
  tab_type: string;
  focused: boolean;
  protected: boolean;
  title?: string;
  agent_type?: string | null;
  agent_session_id?: string | null;
  agent_session_model?: string | null;
  pty_id?: string | null;
  cwd?: string;
}

export interface Workspace {
  id: string;
  name: string;
  is_active: boolean;
  protected: boolean;
  tab_ids: string[];
  tab_count: number;
  cwd?: string;
}

export interface Provider {
  id: string;
  display_name?: string;
  transport?: "pty" | "chat_backend";
  availability?: "available" | "unavailable" | "unknown";
  supports_chat_ui?: boolean;
  supports_permission_bypass?: boolean;
  available_models?: { id: string; display_name: string }[];
  /** Provider-declared in-agent switch template; absent ⇒ no live model switch. */
  model_switch_format?: string | null;
}

export interface PlanSummary {
  plan_id: string;
  title: string;
  status?: string;
}

export type ApiRequestOptions = {
  timeoutMs?: number;
};

function configPort(configPath: string): string | null {
  const m = readFileSync(configPath, "utf8").match(/^port\s*=\s*(\d+)/m);
  return m?.[1] ?? null;
}

function datadirForPort(port: string): string | null {
  const home = os.homedir();
  for (const entry of readdirSync(home)) {
    if (entry !== ".codeterm-dev" && !entry.startsWith(".codeterm-dev-")) continue;
    const configPath = path.join(home, entry, "config.toml");
    if (!existsSync(configPath)) continue;
    if (configPort(configPath) === port) return entry;
  }
  return null;
}

export function devDataDirName(instance = currentDevInstance()): string {
  if (isDevTarget()) return ".codeterm-dev";
  if (isNamedTarget()) {
    const resolved = process.env.CODETERM_DEV_DATA_DIR?.trim();
    if (!resolved || path.basename(resolved) !== instance.dataDirName) {
      throw new Error("PW_TARGET=named requires the bench lib resolved data directory");
    }
    return instance.dataDirName;
  }
  if (process.env.CT_E2E_DATADIR) return process.env.CT_E2E_DATADIR;
  if (instance.name) return instance.dataDirName;
  const port = String(instance.apiPort);
  if (port && port !== "7686") return datadirForPort(port) ?? ".codeterm-dev";
  return ".codeterm-dev";
}

// CT_E2E_DATADIR, or CT_E2E_PORT when its config.toml records that port.
export function readDevToken(instance = currentDevInstance()): string {
  if (isNamedTarget()) {
    const token = process.env.CODETERM_API_TOKEN;
    if (!token?.trim()) throw new Error("PW_TARGET=named requires CODETERM_API_TOKEN from the bench lib resolution");
    return token;
  }
  const dir = devDataDirName(instance);
  const p = path.join(os.homedir(), dir, "config.toml");
  const m = readFileSync(p, "utf8").match(/^token\s*=\s*"([^"]+)"/m);
  if (!m) throw new Error(`no api token in ${p}`);
  return m[1];
}

export class ApiClient {
  private token: string;
  private apiBase: string;

  constructor(token?: string, instance = currentDevInstance()) {
    this.apiBase = instance.apiBase;
    this.token = token ?? readDevToken(instance);
  }

  async request<T>(
    method: string,
    apiPath: string,
    body?: unknown,
    options: ApiRequestOptions = {},
  ): Promise<T> {
    const timeoutMs = options.timeoutMs ?? 15_000;
    if (!Number.isInteger(timeoutMs) || timeoutMs < 1) {
      throw new Error(`invalid API request timeout: ${timeoutMs}`);
    }
    let r: Response;
    try {
      r = await fetch(`${this.apiBase}/api${apiPath}`, {
        method,
        headers: {
          "Content-Type": "application/json",
          Authorization: `Bearer ${this.token}`,
        },
        body: body === undefined ? undefined : JSON.stringify(body),
        signal: AbortSignal.timeout(timeoutMs),
      });
    } catch (error) {
      if (error instanceof Error && (error.name === "TimeoutError" || error.name === "AbortError")) {
        throw new Error(`${method} /api${apiPath} timed out after ${timeoutMs}ms`);
      }
      throw error;
    }
    const text = await r.text();
    if (!r.ok) {
      throw new Error(`${method} /api${apiPath} → HTTP ${r.status}: ${text.slice(0, 300)}`);
    }
    try {
      return JSON.parse(text) as T;
    } catch {
      return text as unknown as T;
    }
  }

  get<T>(p: string, options?: ApiRequestOptions): Promise<T> {
    return this.request<T>("GET", p, undefined, options);
  }
  post<T>(p: string, body?: unknown, options?: ApiRequestOptions): Promise<T> {
    return this.request<T>("POST", p, body, options);
  }

  health(): Promise<{ status: string; build: string; version: string }> {
    return this.get("/health");
  }
  tabs(): Promise<Tab[]> {
    return this.get("/tabs");
  }
  workspaces(): Promise<Workspace[]> {
    return this.get("/workspaces");
  }
  providers(): Promise<Provider[]> {
    return this.get("/agents/providers");
  }

  openEventStream(tabId: string, signal: AbortSignal): Promise<Response> {
    const query = new URLSearchParams({ tab_id: tabId });
    return fetch(`${this.apiBase}/api/events/stream?${query}`, {
      headers: {
        Authorization: `Bearer ${this.token}`,
        Accept: "text/event-stream",
      },
      signal,
    });
  }

  createWorkspace(name: string, cwd?: string): Promise<unknown> {
    return this.post("/workspaces", { name, cwd });
  }
  focusWorkspace(id: string): Promise<unknown> {
    return this.post(`/workspaces/${id}/focus`);
  }
  closeWorkspace(id: string): Promise<unknown> {
    return this.post(`/workspaces/${id}/close`);
  }
  createTab(body?: Record<string, unknown>): Promise<unknown> {
    return this.post("/tabs", body ?? {});
  }
  focusTab(id: string): Promise<unknown> {
    return this.post(`/tabs/${id}/focus`);
  }
  closeTab(id: string): Promise<unknown> {
    return this.post(`/tabs/${id}/close`);
  }
  plans(): Promise<PlanSummary[]> {
    return this.get<{ plans: PlanSummary[] }>("/plans").then((r) => r.plans ?? []);
  }
  deletePlan(id: string): Promise<unknown> {
    return this.request("DELETE", `/plans/${id}`);
  }
}
