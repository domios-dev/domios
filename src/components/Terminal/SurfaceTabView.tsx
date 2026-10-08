import { AppWindow, X } from "lucide-react";
import { PluginView } from "../Extensions/PluginView";
import { pluginViewSurface, surfaceDisplayLabel } from "../../lib/surfaceTab";
import type { ApiConfig, SurfaceTabBacking, Tab } from "../../lib/types";

interface SurfaceTabViewProps {
  tab: Tab;
  backing: SurfaceTabBacking;
  ownerName: string | null;
  onFocusOwner?: () => void;
  onClose: () => void;
  api?: ApiConfig;
  isVisible: boolean;
}

export function SurfaceTabView({ tab, backing, ownerName, onFocusOwner, onClose, api, isVisible }: SurfaceTabViewProps) {
  const label = surfaceDisplayLabel(tab.title, backing, ownerName);
  return (
    <section aria-label={label} className="flex h-full w-full flex-col bg-[var(--ct-bg)]">
      <header className="flex h-8 shrink-0 items-center gap-2 border-b border-[var(--ct-border)] px-3 text-xs text-[var(--ct-fg)]">
        <AppWindow size={13} aria-hidden="true" className="shrink-0 text-[var(--ct-muted-fg)]" />
        <span className="min-w-0 truncate font-medium">{label}</span>
        {ownerName && onFocusOwner && (
          <button
            type="button"
            onClick={onFocusOwner}
            aria-label={`Open owner tab ${ownerName}`}
            className="min-w-0 truncate border-0 bg-transparent p-0 text-[var(--ct-accent)] underline-offset-2 hover:underline cursor-pointer"
          >
            {ownerName}
          </button>
        )}
        <span className="flex-1" />
        <button
          type="button"
          onClick={onClose}
          aria-label={`Close ${label}`}
          className="flex h-6 w-6 shrink-0 items-center justify-center rounded border-0 bg-transparent text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)] cursor-pointer"
        >
          <X size={13} aria-hidden="true" />
        </button>
      </header>
      <div className="relative min-h-0 flex-1">
        <PluginView
          pluginId={backing.plugin_id}
          api={api}
          cwd={tab.cwd}
          active={isVisible}
          surface={pluginViewSurface(tab.id, backing)}
        />
      </div>
    </section>
  );
}
