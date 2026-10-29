import type { FrameIntent } from "../../lib/pluginViewChannel";
import { invokeViewWithConsent } from "../../lib/pluginConsent";
/** Hosts a plugin document over its private, document-bound message port. */
import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { listen, type UnlistenFn } from "../../lib/bridge";
import { type DaemonApiConfig } from "../../lib/webApi";
import { PluginUiHost } from "./PluginUiHost";
import { pluginUiDocumentState, pluginUiHostTransition, pluginUiRuntimeEvent, type PluginUiHostState } from "../../lib/pluginUiHostModel";
import { usePluginDocument } from "./usePluginDocument";
import { invalidatePluginBubble } from "../../lib/pluginBubbleCache";
import { KPI_CHANGE_NOTICE, KPI_RECORDED_EVENT } from "../../lib/kpiChanges";
import { DEFAULT_THEME, themeToCSS } from "../../lib/themeCss";
import { surfaceInvalidationMatches } from "../../lib/surfaceInvalidation";
import { surfaceQuery, type PluginViewSurface } from "../../lib/surfaceTab";
import { createViewHostGate, performViewHostAction, viewHostSupport, type ViewHostGate } from "../../lib/pluginViewHostActions";
import { viewHostDeps } from "../../lib/pluginViewHostDeps";
import { getPluginSnapshotState } from "../../lib/pluginSnapshotStore";

type ApiCfg = DaemonApiConfig;

/** How long the host's "Copied from …" receipt stays on the view. */
const COPY_NOTICE_MS = 2500;
const COPY_NOTICE_STYLE: CSSProperties = {
  position: "absolute",
  left: "50%",
  bottom: 12,
  transform: "translateX(-50%)",
  padding: "4px 10px",
  borderRadius: 999,
  background: "var(--ct-bg-elev, var(--ct-surface))",
  border: "1px solid var(--ct-border-default, rgba(255,255,255,0.12))",
  color: "var(--ct-fg)",
  fontSize: "var(--ct-font-size-sm, 0.8125rem)",
  whiteSpace: "nowrap",
  pointerEvents: "none",
};

/** Every theme token the app derives, so shared components render inside a plugin UI as they do in the app. */
export const PLUGIN_THEME_VARS = Object.keys(themeToCSS(DEFAULT_THEME));

function themeCss(): string {
  const cs = getComputedStyle(document.documentElement);
  return PLUGIN_THEME_VARS.map((v) => `${v}:${cs.getPropertyValue(v).trim()}`)
    .filter((s) => !s.endsWith(":"))
    .join(";");
}

export function PluginView({
  pluginId,
  api: _api,
  onClose,
  embedded,
  cwd,
  active = true,
  focusOnActive = false,
  kpiNotices = false,
  surface,
}: {
  pluginId: string;
  api: ApiCfg | undefined;
  onClose?: () => void;
  /** Render just the iframe (no fullscreen chrome) to embed in a detail panel. */
  embedded?: boolean;
  /** Tab cwd stamped into view-call args that omit one (e.g. the Git panel). */
  cwd?: string | null;
  /** A kept-mounted view that is hidden ignores Escape; the iframe hears `__ctActive` on each change. */
  active?: boolean;
  focusOnActive?: boolean;
  /** Forward KPI ledger commits while shown; a hidden view catches up from its `__ctActive` notice. */
  kpiNotices?: boolean;
  /** Set when the view renders a surface tab; the page reads it from its query string. */
  surface?: PluginViewSurface;
}) {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const channelRef = useRef<ReturnType<typeof usePluginDocument>["channel"]>(undefined);
  const activeRef = useRef(active);
  activeRef.current = active;
  const [runtime, setRuntime] = useState<{ path: string; state: PluginUiHostState } | null>(null);
  // Host actions are fenced by a document generation that only ever grows: hiding the view, a new
  // document, any load of the frame (reload or navigation keeps the same WindowProxy) and unmount
  // end every pending request, which never becomes current again. Each generation gets its own gate.
  const generationRef = useRef(0);
  const hostGate = useRef<ViewHostGate>(createViewHostGate());
  const invalidateHostActions = useCallback(() => {
    generationRef.current += 1;
    hostGate.current = createViewHostGate();
  }, []);
  // The host's own, non-suppressible receipt for a clipboard write the plugin asked for.
  // A fresh object per copy, so a repeated copy restarts the notice instead of being swallowed.
  const [copiedFrom, setCopiedFrom] = useState<{ name: string } | null>(null);
  const announceCopy = useCallback(() => {
    const plugin = getPluginSnapshotState().snapshot.plugins.find((item) => item.id === pluginId);
    setCopiedFrom({ name: plugin?.display_name || pluginId });
  }, [pluginId]);
  useEffect(() => {
    if (!copiedFrom) return;
    const timer = setTimeout(() => setCopiedFrom(null), COPY_NOTICE_MS);
    return () => clearTimeout(timer);
  }, [copiedFrom]);
  useEffect(() => {
    if (!active) invalidateHostActions();
  }, [active, invalidateHostActions]);

  // A hot-reloaded plugin ships a new bundle, but the iframe keeps the one it
  // already loaded until its src changes, so the reload has to bust it.
  const [reloadNonce, setReloadNonce] = useState(0);
  useEffect(() => {
    let alive = true;
    let unlisten: UnlistenFn | undefined;
    listen<{ kind: string }>("contributions-changed", (event) => {
      if (alive && event.payload.kind === `plugin_view:${pluginId}`) {
        setReloadNonce((n) => n + 1);
      }
    })
      .then((fn) => {
        if (alive) unlisten = fn;
        else fn();
      })
      .catch(() => undefined);
    return () => {
      alive = false;
      unlisten?.();
    };
  }, [pluginId]);

  useEffect(() => {
    if (!surface) return;
    let alive = true;
    let unlisten: UnlistenFn | undefined;
    listen<unknown>("surface-state-invalidated", (event) => {
      if (alive && surfaceInvalidationMatches(event.payload, pluginId, surface)) {
        channelRef.current?.send({ __ctSurfaceStateChanged: true });
      }
    }).then((release) => {
      if (alive) unlisten = release;
      else release();
    }).catch(() => undefined);
    return () => { alive = false; unlisten?.(); };
  }, [pluginId, surface?.surfaceId, surface?.instanceId, surface?.tabId]);

  const path = `/api/plugins/${encodeURIComponent(pluginId)}/ui/index.html${surfaceQuery(surface).replace(/^&/, "?")}`;
  const documentPath = `${path}${path.includes("?") ? "&" : "?"}reload=${reloadNonce}`;
  const { channel, html, invoke: invokeView, media, dispose: disposeDocument, frameToken, error: documentError } = usePluginDocument(documentPath);
  channelRef.current = channel;
  useEffect(() => invalidateHostActions, [documentPath, frameToken, invalidateHostActions]);
  const callDocument = useMemo(() => ({ live: true, epoch: 0 }), [documentPath, invokeView]);
  const currentCallDocument = useRef(callDocument);
  currentCallDocument.current = callDocument;
  useEffect(() => {
    callDocument.live = true;
    callDocument.epoch += 1;
    return () => { callDocument.live = false; callDocument.epoch += 1; };
  }, [callDocument]);
  const state = pluginUiDocumentState(html, documentError, runtime?.path === documentPath ? runtime.state : { status: "loading" });

  const onMessage = useCallback(
    async (m: Record<string, unknown>, intent: FrameIntent) => {
      const win = iframeRef.current?.contentWindow;
      if (!win || !channel) return;
      const event = pluginUiRuntimeEvent(m.__ctUiState);
      if (event) {
        setRuntime((current) => ({ path: documentPath, state: pluginUiHostTransition(current?.path === documentPath ? current.state : { status: "loading" }, event) }));
        return;
      }
      if (m.__ctClose) {
        onClose?.();
        return;
      }
      if (m.__ctHost != null) {
        // The request belongs to this document generation; hide, reload, navigation and disposal end it for good.
        const generation = intent.generation;
        const isCurrent = () => generationRef.current === generation && iframeRef.current?.contentWindow === win && activeRef.current;
        const deps = viewHostDeps();
        const writeText = deps.writeText;
        const announced = writeText && {
          ...deps,
          writeText: async (text: string) => {
            await writeText(text);
            announceCopy();
          },
        };
        const result = await performViewHostAction(pluginId, frameToken ?? null, m, intent, announced || deps, isCurrent, hostGate.current);
        if (isCurrent()) channel.send({ __ctReply: m.__ctHost, result });
        return;
      }
      if (m.__ctMedia != null) {
        try {
          if (!media || !Number.isSafeInteger(m.__ctMedia) || typeof m.id !== "string") throw new Error("Plugin media unavailable");
          const result = await media(m.id);
          if (iframeRef.current?.contentWindow === win) channel.send({ __ctReply: m.__ctMedia, result });
        } catch {
          if (iframeRef.current?.contentWindow === win) channel.send({ __ctReply: m.__ctMedia, error: "Plugin media unavailable" });
        }
        return;
      }
      if (m.__ctInvoke != null) {
        if (!Number.isSafeInteger(m.__ctInvoke)) return;
        const requestEpoch = callDocument.epoch;
        const isCurrent = () => callDocument.live && callDocument.epoch === requestEpoch && currentCallDocument.current === callDocument && iframeRef.current?.contentWindow === win;
        try {
          // iframes (e.g. Git) never need to know their own path.
          const args =
            cwd && m.args && typeof m.args === "object" && (m.args as { cwd?: unknown }).cwd == null
              ? { ...m.args, cwd }
              : m.args;
          const method = m.method;
          if (!isCurrent() || !invokeView || typeof method !== "string") return;
          const result = await invokeViewWithConsent(pluginId, () => invokeView(method, args), isCurrent);
          if (!isCurrent()) return;
          // In-panel actions invalidate their bubble immediately.
          const usedCwd = (args as { cwd?: unknown } | undefined)?.cwd;
          if (typeof usedCwd === "string") invalidatePluginBubble(pluginId, usedCwd);
          channel.send({ __ctReply: m.__ctInvoke, result });
        } catch (err) {
          if (!isCurrent()) return;
          channel.send({ __ctReply: m.__ctInvoke, error: String(err) });
        }
      }
    },
    [pluginId, cwd, onClose, documentPath, surface, media, frameToken, invokeView, callDocument, announceCopy, channel],
  );

  const captureIntent = useCallback((): FrameIntent => ({
    shown: activeRef.current,
    frameFocused: iframeRef.current !== null && document.activeElement === iframeRef.current,
    activated: globalThis.navigator?.userActivation?.isActive === true,
    generation: generationRef.current,
  }), []);
  const bindFrame = useCallback((frame: HTMLIFrameElement | null) => {
    iframeRef.current = frame;
    if (frame?.contentWindow) channel?.bind(frame.contentWindow, captureIntent);
  }, [channel, captureIntent]);
  useEffect(() => channel?.subscribe(onMessage, () => {
    channel.send({ __ctTheme: themeCss() });
    channel.send({ __ctActive: activeRef.current });
    channel.send({ __ctHostSupport: viewHostSupport(viewHostDeps()) });
    if (surface) channel.send({ __ctSurfaceStateChanged: true });
  }, () => {
    callDocument.live = false;
    callDocument.epoch += 1;
    invalidateHostActions();
    disposeDocument?.();
    setRuntime({ path: documentPath, state: { status: "error", reason: "Plugin document changed. Reopen the view to continue." } });
  }), [channel, onMessage, surface, callDocument, invalidateHostActions, disposeDocument, documentPath]);

  useEffect(() => {
    if (!kpiNotices) return;
    let alive = true;
    let unlisten: UnlistenFn | undefined;
    listen(KPI_RECORDED_EVENT, () => {
      if (activeRef.current) channelRef.current?.send({ [KPI_CHANGE_NOTICE]: true });
    })
      .then((fn) => {
        if (alive) unlisten = fn;
        else fn();
      })
      .catch(() => undefined);
    return () => {
      alive = false;
      unlisten?.();
    };
  }, [kpiNotices]);

  useEffect(() => {
    channel?.send({ __ctActive: active });
    if (active && focusOnActive && !embedded && state.status === "ready") iframeRef.current?.focus();
  }, [active, focusOnActive, embedded, state.status, channel]);

  // Universal escape (host-side focus); in-iframe Esc/close routes via __ctClose.
  useEffect(() => {
    if (embedded || !active) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose?.();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [active, embedded, onClose]);

  const frame = (
    <PluginUiHost pluginId={pluginId} state={state} onRetry={() => setReloadNonce((n) => n + 1)}>
      {html !== undefined && html.trim() && <iframe
        key={documentPath}
        ref={bindFrame}
        data-testid="extensions-plugin-iframe"
        data-plugin-id={pluginId}
        srcDoc={html}
        onLoad={() => {
          invalidateHostActions();
          channel?.observeLoad();
        }}
        title={`${pluginId} view`}
        sandbox="allow-scripts"
        style={{ flex: 1, width: "100%", border: "none", background: "var(--ct-bg)", display: "block", height: "100%" }}
      />}
      {copiedFrom && <div role="status" data-testid="plugin-view-copied" style={COPY_NOTICE_STYLE}>Copied from {copiedFrom.name}</div>}
    </PluginUiHost>
  );

  if (embedded) {
    return (
      <div
        data-testid="extensions-plugin-embedded-view"
        data-plugin-id={pluginId}
        style={{
          width: "100%",
          height: 360,
          display: "flex",
          borderRadius: 8,
          overflow: "hidden",
          background: "var(--ct-bg)",
          border: "1px solid var(--ct-border-subtle, rgba(255,255,255,0.08))",
        }}
      >
        {frame}
      </div>
    );
  }

  return (
    <div style={{ position: "absolute", inset: 0, display: "flex", flexDirection: "column", background: "var(--ct-bg)" }}>
      {frame}
    </div>
  );
}
