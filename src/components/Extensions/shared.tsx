// Small shared bits for the Extensions surface (labels, API base, error parsing,
// icon adapter). Kept here so the list/detail/secret-backend views don't each
// re-roll them.
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

export function apiBaseOf(config: AppConfigShape | null): { base: string; headers: Record<string, string> } {
  const api = config?.api;
  const base = daemonHttpBase(api);
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  const token = daemonAuthToken(api);
  if (token) headers.Authorization = `Bearer ${token}`;
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
