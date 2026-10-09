import { useMemo } from "react";
import { PluginView } from "../Extensions/PluginView";
import { ProviderGlyph } from "../ProviderGlyph";
import { SurfaceFlowLine } from "../Surface/SurfaceFlowLine";
import { surfaceIconComponent } from "../Surface/surfaceIcon";
import { createHttpSseSurfaceTransport, type HttpSseSurfaceTransportOptions } from "../Surface/httpSseSurfaceTransport";
import { downloadScreenshot, StreamSurfaceView } from "../Surface/StreamSurfaceView";
import type { SurfaceChromeSpec } from "../Surface/surfaceChromeModel";
import { screenshotSaveMode, surfaceRenderPlan, type SurfaceDeclLookup } from "../Surface/surfaceRenderPlan";
import type { SurfaceStreamTransport } from "../Surface/surfaceStreamTransport";
import { pluginViewSurface, surfaceDisplayLabel } from "../../lib/surfaceTab";
import type { ApiConfig, SurfaceTabBacking, Tab } from "../../lib/types";
import { useTabProvider } from "../../lib/useTabProvider";
import { daemonAuthToken, daemonHttpBase, IS_WEB } from "../../lib/webApi";

export type SurfaceTransportFactory = (options: HttpSseSurfaceTransportOptions) => SurfaceStreamTransport;

interface SurfaceTabViewProps {
  tab: Tab;
  backing: SurfaceTabBacking;
  ownerName: string | null;
  /** The owner's tab, for its provider glyph in the breadcrumb. */
  ownerTab?: Tab | null;
  onFocusOwner?: () => void;
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
      surfaceTabId={tab.id}
      saveScreenshot={screenshotSaveMode(IS_WEB) === "download" ? downloadScreenshot : keepSavedPath}
    />
  );
}

function OwnerChip({ ownerName, ownerTab, onFocusOwner }: { ownerName: string; ownerTab: Tab | null; onFocusOwner?: () => void }) {
  const provider = useTabProvider(ownerTab);
  const content = (
    <>
      {provider && (
        <ProviderGlyph
          iconSvg={provider.iconSvg}
          iconColor={provider.iconColor}
          iconHint={provider.iconHint}
          className="shrink-0 text-[11px] leading-none"
          title={provider.displayName}
        />
      )}
      <span className="min-w-0 truncate">{ownerName}</span>
    </>
  );
  if (!onFocusOwner) {
    return <span data-testid="surface-owner-chip" className="ct-surface-owner-chip">{content}</span>;
  }
  return (
    <button
      type="button"
      data-testid="surface-owner-chip"
      onClick={onFocusOwner}
      aria-label={`Open owner tab ${ownerName}`}
      className="ct-surface-owner-chip ct-surface-owner-chip--link"
    >
      {content}
    </button>
  );
}

export function SurfaceTabView({
  tab,
  backing,
  ownerName,
  ownerTab = null,
  onFocusOwner,
  api,
  isVisible,
  decl,
  createTransport = createHttpSseSurfaceTransport,
}: SurfaceTabViewProps) {
  const label = surfaceDisplayLabel(tab.title, backing, ownerName);
  const plan = surfaceRenderPlan(decl, isVisible);
  const SurfaceGlyph = surfaceIconComponent(decl.status === "ready" ? decl.declared : null, { pluginId: backing.plugin_id, surfaceId: backing.surface_id });
  return (
    <section aria-label={label} className="flex h-full w-full flex-col bg-[var(--ct-bg)]">
      <header className="flex h-8 shrink-0 items-center gap-2 bg-[var(--ct-titlebar-bg)] px-3 text-xs text-[var(--ct-fg)]">
        <nav aria-label="Surface breadcrumb" data-testid="surface-breadcrumb" className="flex min-w-0 flex-1 items-center gap-1.5">
          {ownerName && (
            <>
              <OwnerChip ownerName={ownerName} ownerTab={ownerTab} onFocusOwner={onFocusOwner} />
              <span aria-hidden="true" className="shrink-0 text-[var(--ct-muted)]">›</span>
            </>
          )}
          <span data-testid="surface-crumb" className="flex min-w-0 items-center gap-1.5">
            <SurfaceGlyph data-testid="surface-crumb-icon" size={13} aria-hidden="true" className="shrink-0 text-[var(--ct-muted-fg)]" />
            <span className="min-w-0 truncate font-medium">{ownerName ? tab.title : label}</span>
          </span>
        </nav>
      </header>
      {plan.kind === "view" && <SurfaceFlowLine surfaceTabId={tab.id} />}
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
