import { useMemo } from "react";
import { PluginView } from "../Extensions/PluginView";
import { ProviderGlyph } from "../ProviderGlyph";
import { SurfaceFlowLine } from "../Surface/SurfaceFlowLine";
import { surfaceIconComponent } from "../Surface/surfaceIcon";
import { createHttpSseSurfaceTransport, type HttpSseSurfaceTransportOptions } from "../Surface/httpSseSurfaceTransport";
import { surfaceTransportTarget } from "../Surface/surfaceTransportTarget";
import { StreamSurfaceView } from "../Surface/StreamSurfaceView";
import type { SurfaceChromeSpec } from "../Surface/surfaceChromeModel";
import { surfaceRenderPlan, type SurfaceDeclLookup } from "../Surface/surfaceRenderPlan";
import type { SurfaceStreamTransport } from "../Surface/surfaceStreamTransport";
import { invoke } from "../../lib/bridge";
import { useSurfaceActor } from "../../lib/surfaceActivity";
import { pluginViewSurface, surfaceDisplayLabel } from "../../lib/surfaceTab";
import type { ApiConfig, SurfaceTabBacking, Tab } from "../../lib/types";
import { useTabProvider } from "../../lib/useTabProvider";
import { daemonAuthToken, daemonHttpBase, relayTarget } from "../../lib/webApi";

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
  onRetry?: () => void;
  createTransport?: SurfaceTransportFactory;
}

interface StreamBodyProps {
  visible: boolean;
  tab: Tab;
  api?: ApiConfig;
  ownerName: string | null;
  chrome: SurfaceChromeSpec;
  createTransport: SurfaceTransportFactory;
}

export const openInSystemBrowser = (url: string) => {
  invoke("open_url", { url }).catch((error: unknown) => console.warn("[surface] open in browser failed", error));
};

function StreamSurfaceBody({ visible, tab, api, ownerName, chrome, createTransport }: StreamBodyProps) {
  const baseUrl = daemonHttpBase(api);
  const token = daemonAuthToken(api);
  const { tabId, peerId } = surfaceTransportTarget(tab, relayTarget()?.id ?? null);
  const transport = useMemo(() => createTransport({ baseUrl, token, tabId, ...(peerId ? { peerId } : {}) }), [createTransport, baseUrl, token, tabId, peerId]);
  return (
    <StreamSurfaceView
      visible={visible}
      transport={transport}
      chrome={chrome}
      surfaceTitle={tab.title}
      initialState={{ owner: ownerName ?? "" }}
      ownerName={ownerName}
      surfaceTabId={tab.id}
      openExternal={openInSystemBrowser}
    />
  );
}

/** "· QA" while another agent drives the owner's surface. */
function SurfaceActorTag({ surfaceTabId }: { surfaceTabId: string }) {
  const actor = useSurfaceActor(surfaceTabId);
  if (!actor) return null;
  return (
    <span data-testid="surface-actor" title={`${actor} is using this surface`} className="min-w-0 shrink truncate text-[var(--ct-muted-fg)]">
      · {actor}
    </span>
  );
}

function SurfaceUnavailable({ title, error, onRetry }: { title: string; error?: string; onRetry?: () => void }) {
  return (
    <div role="status" data-testid="surface-unavailable" className="flex h-full flex-col items-center justify-center gap-2 p-4 text-center text-sm text-[var(--ct-muted-fg)]">
      <p>{error ? `${title}: could not reach the surface registry` : `${title}: no surface is registered`}</p>
      {onRetry && <button type="button" onClick={onRetry}>Retry surface lookup</button>}
      {error && (
        <details className="max-w-full text-xs">
          <summary className="cursor-pointer select-none">Details</summary>
          <code className="mt-1 block break-all">{error}</code>
        </details>
      )}
    </div>
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
  onRetry,
  createTransport = createHttpSseSurfaceTransport,
}: SurfaceTabViewProps) {
  const label = surfaceDisplayLabel(tab.title, backing, ownerName);
  const plan = surfaceRenderPlan(decl, isVisible);
  const SurfaceGlyph = surfaceIconComponent(decl.status === "ready" ? decl.declared : null, { pluginId: backing.plugin_id, surfaceId: backing.surface_id });
  return (
    <section aria-label={label} className="flex h-full w-full flex-col bg-[var(--ct-bg)]">
      <header className="ct-jobs-header flex h-8 shrink-0 items-center gap-2 px-3 text-xs text-[var(--ct-fg)]">
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
            <SurfaceActorTag surfaceTabId={tab.id} />
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
        {plan.kind === "stream" && (
          <StreamSurfaceBody visible={plan.live} tab={tab} api={api} ownerName={ownerName} chrome={plan.chrome} createTransport={createTransport} />
        )}
        {plan.kind === "unavailable" && <SurfaceUnavailable title={tab.title || backing.surface_id} error={plan.error} onRetry={onRetry} />}
        {plan.kind === "unsupported" && (
          <p role="alert" className="p-4 text-sm text-[var(--ct-muted-fg)]">{plan.message}</p>
        )}
      </div>
    </section>
  );
}
