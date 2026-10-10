import { BROWSER_SURFACE_CHROME } from "./browserChrome";
import type { SurfaceChromeSpec, ToolbarInvoke, ToolbarItemSpec } from "./surfaceChromeModel";

export interface DeclaredSurface {
  pluginId: string;
  surfaceId: string;
  renderer: string;
  runtime: string | null;
  chrome: unknown;
  icon?: string;
}

export type SurfaceDeclLookup =
  | { status: "loading" }
  | { status: "ready"; declared: DeclaredSurface | null }
  | { status: "failed"; error: string };

export type SurfaceRenderPlan =
  | { kind: "pending" }
  | { kind: "view" }
  | { kind: "unavailable"; error?: string }
  | { kind: "stream"; runtime: string; chrome: SurfaceChromeSpec; live: boolean }
  | { kind: "unsupported"; message: string };

const ITEM_KINDS: ReadonlySet<string> = new Set(["button", "input", "select"]);

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function stringField(value: Record<string, unknown>, key: string): string | undefined {
  return typeof value[key] === "string" ? (value[key] as string) : undefined;
}

function declaredRow(value: unknown): DeclaredSurface | null {
  if (!isRecord(value)) return null;
  const pluginId = stringField(value, "plugin_id");
  const surfaceId = stringField(value, "surface_id");
  if (!pluginId || !surfaceId) return null;
  const row: DeclaredSurface = {
    pluginId,
    surfaceId,
    renderer: stringField(value, "renderer") ?? "view",
    runtime: stringField(value, "runtime") ?? null,
    chrome: value.chrome,
  };
  const icon = stringField(value, "icon");
  if (icon) row.icon = icon;
  return row;
}

export function parseDeclaredSurfaces(body: unknown): DeclaredSurface[] {
  if (!isRecord(body) || !Array.isArray(body.declared)) return [];
  return body.declared.map(declaredRow).filter((row): row is DeclaredSurface => row !== null);
}

export function findDeclaredSurface(declared: readonly DeclaredSurface[], pluginId: string, surfaceId: string): DeclaredSurface | null {
  return declared.find((row) => row.pluginId === pluginId && row.surfaceId === surfaceId) ?? null;
}

function toolbarInvoke(value: unknown): ToolbarInvoke | undefined {
  if (!isRecord(value) || typeof value.method !== "string") return undefined;
  const invoke: ToolbarInvoke = { method: value.method };
  if (isRecord(value.args)) invoke.args = value.args;
  if (typeof value.valueArg === "string") invoke.valueArg = value.valueArg;
  return invoke;
}

function toolbarItem(value: unknown): ToolbarItemSpec | null {
  if (!isRecord(value)) return null;
  const id = stringField(value, "id");
  const label = stringField(value, "label");
  const kind = stringField(value, "kind");
  if (!id || !label || !kind || !ITEM_KINDS.has(kind)) return null;
  const item: ToolbarItemSpec = { id, label, kind: kind as ToolbarItemSpec["kind"] };
  for (const key of ["icon", "placeholder", "source"] as const) {
    const field = stringField(value, key);
    if (field !== undefined) item[key] = field;
  }
  if (value.when === "interactive" || value.when === "always") item.when = value.when;
  const invoke = toolbarInvoke(value.invoke);
  if (invoke) item.invoke = invoke;
  return item;
}

// Malformed toolbar items are dropped one by one so a single typo does not cost the whole chrome.
export function parseChromeSpec(value: unknown): SurfaceChromeSpec | null {
  if (!isRecord(value)) return null;
  const toolbar = Array.isArray(value.toolbar) ? value.toolbar.map(toolbarItem).filter((item): item is ToolbarItemSpec => item !== null) : [];
  const spec: SurfaceChromeSpec = { toolbar };
  for (const flag of ["takeControl", "pointAndAsk", "snip", "openExternal"] as const) {
    if (typeof value[flag] === "boolean") spec[flag] = value[flag] as boolean;
  }
  // The camera was declared as `screenshot` before it became snip and ask.
  if (spec.snip === undefined && typeof value.screenshot === "boolean") spec.snip = value.screenshot;
  return spec;
}

export function surfaceRenderPlan(lookup: SurfaceDeclLookup, visible: boolean): SurfaceRenderPlan {
  if (lookup.status === "loading") return { kind: "pending" };
  // Without a declaration the renderer is unknown and the plugin's view route 404s, so neither can be mounted.
  if (lookup.status === "failed") return { kind: "unavailable", error: lookup.error };
  if (!lookup.declared) return { kind: "unavailable" };
  const { renderer, runtime, chrome } = lookup.declared;
  if (renderer === "view") return { kind: "view" };
  if (renderer !== "stream") return { kind: "unsupported", message: `This surface uses the "${renderer}" renderer, which this Domios cannot show.` };
  if (!runtime) return { kind: "unsupported", message: "This stream surface declares no runtime." };
  return { kind: "stream", runtime, chrome: parseChromeSpec(chrome) ?? BROWSER_SURFACE_CHROME, live: visible };
}
