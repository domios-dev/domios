import { useCallback, useSyncExternalStore } from "react";
import {
  AppWindow,
  BarChart3,
  Code,
  FileText,
  Globe,
  Image,
  LayoutDashboard,
  Map as MapIcon,
  Monitor,
  Play,
  SquareTerminal,
  type LucideIcon,
} from "lucide-react";
import type { SurfaceTabBacking } from "../../lib/types";

const SURFACE_ICONS: Readonly<Record<string, LucideIcon>> = {
  globe: Globe,
  browser: Globe,
  "app-window": AppWindow,
  monitor: Monitor,
  terminal: SquareTerminal,
  "square-terminal": SquareTerminal,
  "layout-dashboard": LayoutDashboard,
  dashboard: LayoutDashboard,
  "file-text": FileText,
  image: Image,
  map: MapIcon,
  code: Code,
  "bar-chart": BarChart3,
  chart: BarChart3,
  play: Play,
};

export interface SurfaceIconHint {
  icon?: string | null;
  runtime?: string | null;
}

/** Declared icon first, then the runtime; ids are the last resort before the declaration loads. */
export function surfaceIconComponent(hint: SurfaceIconHint | null, ids?: { pluginId: string; surfaceId: string }): LucideIcon {
  const named = hint?.icon ? SURFACE_ICONS[hint.icon.trim().toLowerCase()] : undefined;
  if (named) return named;
  if (hint?.runtime === "browser") return Globe;
  if (!hint && ids && /browser/i.test(`${ids.pluginId} ${ids.surfaceId}`)) return Globe;
  return AppWindow;
}

const hints = new Map<string, SurfaceIconHint>();
const listeners = new Set<() => void>();
const hintKey = (pluginId: string, surfaceId: string) => `${pluginId}\u0000${surfaceId}`;

export function rememberDeclaredSurfaces(declared: readonly { pluginId: string; surfaceId: string; runtime: string | null; icon?: string }[]): void {
  let changed = false;
  for (const row of declared) {
    const key = hintKey(row.pluginId, row.surfaceId);
    const previous = hints.get(key);
    if (previous?.icon === row.icon && previous?.runtime === row.runtime) continue;
    hints.set(key, { icon: row.icon ?? null, runtime: row.runtime });
    changed = true;
  }
  if (changed) for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useSurfaceIcon(backing: Pick<SurfaceTabBacking, "plugin_id" | "surface_id"> | null): LucideIcon {
  const pluginId = backing?.plugin_id ?? "";
  const surfaceId = backing?.surface_id ?? "";
  const read = useCallback(() => hints.get(hintKey(pluginId, surfaceId)) ?? null, [pluginId, surfaceId]);
  const hint = useSyncExternalStore(subscribe, read, read);
  return surfaceIconComponent(hint, { pluginId, surfaceId });
}

interface SurfaceIconProps {
  backing: Pick<SurfaceTabBacking, "plugin_id" | "surface_id"> | null;
  size: number;
  className?: string;
  "data-testid"?: string;
}

export function SurfaceIcon({ backing, size, className, "data-testid": testId }: SurfaceIconProps) {
  const Icon = useSurfaceIcon(backing);
  return <Icon data-testid={testId} aria-hidden="true" size={size} className={className} />;
}
