import { useMemo } from "react";
import { AppWindow, X } from "lucide-react";
import { PluginView } from "../Extensions/PluginView";
import { createHttpSseSurfaceTransport, type HttpSseSurfaceTransportOptions } from "../Surface/httpSseSurfaceTransport";
import { downloadScreenshot, StreamSurfaceView } from "../Surface/StreamSurfaceView";
import type { SurfaceChromeSpec } from "../Surface/surfaceChromeModel";
import { screenshotSaveMode, surfaceRenderPlan, type SurfaceDeclLookup } from "../Surface/surfaceRenderPlan";
import type { SurfaceStreamTransport } from "../Surface/surfaceStreamTransport";
import { pluginViewSurface, surfaceDisplayLabel } from "../../lib/surfaceTab";
import type { ApiConfig, SurfaceTabBacking, Tab } from "../../lib/types";
import { daemonAuthToken, daemonHttpBase, IS_WEB } from "../../lib/webApi";

export type SurfaceTransportFactory = (options: HttpSseSurfaceTransportOptions) => SurfaceStreamTransport;

interface SurfaceTabViewProps {
  tab: Tab;
  backing: SurfaceTabBacking;
  ownerName: string | null;
  onFocusOwner?: () => void;
  onClose: () => void;
  api?: ApiConfig;
  isVisible: boolean;
  decl: SurfaceDeclLookup;
  createTransport?: SurfaceTransportFactory;
}

interface StreamBodyProps {
  tab: Tab;
  api?: ApiConfig;
  ownerName: string | null;
  chrome: SurfaceChromeSpec;
  createTransport: SurfaceTransportFactory;
}

// The core already wrote the file; the view's "Saved <path>" notice is the whole desktop save.
const keepSavedPath = () => undefined;

function StreamSurfaceBody({ tab, api, ownerName, chrome, createTransport }: StreamBodyProps) {
  const baseUrl = daemonHttpBase(api);
  const token = daemonAuthToken(api);
  const transport = useMemo(() => createTransport({ baseUrl, token, tabId: tab.id }), [createTransport, baseUrl, token, tab.id]);
  return (
    <StreamSurfaceView
      transport={transport}
      chrome={chrome}
      surfaceTitle={tab.title}
      initialState={{ owner: ownerName ?? "" }}
      ownerName={ownerName}
      saveScreenshot={screenshotSaveMode(IS_WEB) === "download" ? downloadScreenshot : keepSavedPath}
    />
  );
}

export function SurfaceTabView({
  tab,
  backing,
  ownerName,
  onFocusOwner,
  onClose,
  api,
  isVisible,
  decl,
  createTransport = createHttpSseSurfaceTransport,
}: SurfaceTabViewProps) {
  const label = surfaceDisplayLabel(tab.title, backing, ownerName);
  const plan = surfaceRenderPlan(decl, isVisible);
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
      <div className="relative min-h-0 flex-1" aria-busy={plan.kind === "pending"}>
        {plan.kind === "view" && (
          <PluginView
            pluginId={backing.plugin_id}
            api={api}
            cwd={tab.cwd}
            active={isVisible}
            surface={pluginViewSurface(tab.id, backing)}
          />
        )}
        {plan.kind === "stream" && plan.live && (
          <StreamSurfaceBody tab={tab} api={api} ownerName={ownerName} chrome={plan.chrome} createTransport={createTransport} />
        )}
        {plan.kind === "unsupported" && (
          <p role="alert" className="p-4 text-sm text-[var(--ct-muted-fg)]">{plan.message}</p>
        )}
      </div>
    </section>
  );
}
