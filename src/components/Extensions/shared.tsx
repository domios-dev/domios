import type { CatalogEntry } from "./pluginCatalog";
import type { PluginIconData } from "./PluginIcon";
import { daemonHttpBase, daemonAuthToken } from "../../lib/webApi";

export interface AppConfigShape {
  plugins?: Record<string, { pinned?: boolean; serverUrl?: string; [k: string]: unknown } | undefined>;
  integrations?: {
    secrets?: { backend?: string; default_organization?: string; default_collection?: string };
  };
  agents?: Record<string, unknown>;
  api?: { host?: string; port?: number; token?: string };
  [k: string]: unknown;
}

// Headers depend only on the auth token, so a token keeps the same headers
// object across calls — callers use it as a stable effect dependency instead
const headersByToken = new Map<string, Record<string, string>>();

export function apiBaseOf(config: AppConfigShape | null): { base: string; headers: Record<string, string> } {
  const api = config?.api;
  const base = daemonHttpBase(api);
  const token = daemonAuthToken(api);
  let headers = headersByToken.get(token);
  if (!headers) {
    headers = { "Content-Type": "application/json" };
    if (token) headers.Authorization = `Bearer ${token}`;
    headersByToken.set(token, headers);
  }
  return { base, headers };
}

// Secret API errors come back either flat ({message}) or nested ({error:{message}}).
export async function readApiError(r: Response, fallback: string): Promise<string> {
  const j = (await r.json().catch(() => null)) as { message?: string; error?: { message?: string } } | null;
  return j?.error?.message ?? j?.message ?? `${fallback} (${r.status})`;
}

/** Adapt a catalog entry's camelCase icon fields back to the PluginIcon shape. */
export function iconOf(entry: Pick<CatalogEntry, "iconSvg" | "iconColor" | "iconHint">): PluginIconData {
  return { icon_svg: entry.iconSvg, icon_color: entry.iconColor, icon_hint: entry.iconHint };
}

export function GroupLabel({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        fontFamily: '"JetBrains Mono", ui-monospace, monospace',
        fontSize: 10.5,
        letterSpacing: "0.1em",
        textTransform: "uppercase",
        color: "var(--ct-muted)",
        margin: "0 0 8px 2px",
      }}
    >
      {children}
    </div>
  );
}
