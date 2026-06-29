/**
 * Typed wrapper over the dev console's REST API (port 7686).
 * Token comes from ~/.codeterm-dev/config.toml — same source the CLI uses.
 */
import { existsSync, readFileSync, readdirSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { API_BASE } from "./dev-console";

export interface Pane {
  id: string;
  pane_type: string;
  focused: boolean;
  protected: boolean;
  title?: string;
  agent_type?: string | null;
  pty_id?: string | null;
  cwd?: string;
}

export interface Workspace {
  id: string;
  name: string;
  is_active: boolean;
  protected: boolean;
  pane_ids: string[];
  tab_count: number;
  cwd?: string;
}

export interface Provider {
  id: string;
  available_models?: { id: string; display_name: string }[];
}

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

export function devDataDirName(): string {
  if (process.env.CT_E2E_DATADIR) return process.env.CT_E2E_DATADIR;
  const port = process.env.CT_E2E_PORT;
  if (port && port !== "7686") return datadirForPort(port) ?? ".codeterm-dev";
  return ".codeterm-dev";
}

// Default data dir is ~/.codeterm-dev. An isolated instance can override via
// CT_E2E_DATADIR, or CT_E2E_PORT when its config.toml records that port.
export function readDevToken(): string {
  const dir = devDataDirName();
  const p = path.join(os.homedir(), dir, "config.toml");
  const m = readFileSync(p, "utf8").match(/^token\s*=\s*"([^"]+)"/m);
  if (!m) throw new Error(`no api token in ${p}`);
  return m[1];
}

export class ApiClient {
  private token: string;

  constructor(token = readDevToken()) {
    this.token = token;
  }

  async request<T>(method: string, apiPath: string, body?: unknown): Promise<T> {
    const r = await fetch(`${API_BASE}/api${apiPath}`, {
      method,
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${this.token}`,
      },
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: AbortSignal.timeout(15_000),
    });
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

  get<T>(p: string): Promise<T> {
    return this.request<T>("GET", p);
  }
  post<T>(p: string, body?: unknown): Promise<T> {
    return this.request<T>("POST", p, body);
  }

  health(): Promise<{ status: string; build: string; version: string }> {
    return this.get("/health");
  }
  panes(): Promise<Pane[]> {
    return this.get("/panes");
  }
  workspaces(): Promise<Workspace[]> {
    return this.get("/workspaces");
  }
  providers(): Promise<Provider[]> {
    return this.get("/agents/providers");
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
  createPane(body?: Record<string, unknown>): Promise<unknown> {
    return this.post("/panes", body ?? {});
  }
  focusPane(id: string): Promise<unknown> {
    return this.post(`/panes/${id}/focus`);
  }
  closePane(id: string): Promise<unknown> {
    return this.post(`/panes/${id}/close`);
  }
}
