/**
 * Host for a plugin's iframe UI (the `view` capability). Renders the plugin's
 * `ui/index.html` in a sandboxed iframe (origin-null, CSP-locked) and relays the
 * `ct.invoke` postMessage bridge to the permission-gated `plugin_view_call`.
 *
 * The iframe holds no capability: its only channel is postMessage to us, and we
 * forward each call to the host (which stamps the plugin id + checks permissions).
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, listen, type UnlistenFn } from "../../lib/bridge";
import { type DaemonApiConfig } from "../../lib/webApi";
import { PluginUiHost } from "./PluginUiHost";
import { pluginUiDocumentState, pluginUiHostTransition, pluginUiRuntimeEvent, type PluginUiHostState } from "../../lib/pluginUiHostModel";
import { usePluginDocument } from "./usePluginDocument";
import { invalidatePluginBubble } from "../../lib/pluginBubbleCache";
import { KPI_CHANGE_NOTICE, KPI_RECORDED_EVENT } from "../../lib/kpiChanges";
import { DEFAULT_THEME, themeToCSS } from "../../lib/themeCss";
import { surfaceQuery, type PluginViewSurface } from "../../lib/surfaceTab";

type ApiCfg = DaemonApiConfig;

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
  const activeRef = useRef(active);
  activeRef.current = active;
  const [runtime, setRuntime] = useState<{ path: string; state: PluginUiHostState } | null>(null);

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

  const path = `/api/plugins/${encodeURIComponent(pluginId)}/ui/index.html${surfaceQuery(surface).replace(/^&/, "?")}`;
  const documentPath = `${path}${path.includes("?") ? "&" : "?"}reload=${reloadNonce}`;
  const { html, error: documentError } = usePluginDocument(documentPath);
  const state = pluginUiDocumentState(html, documentError, runtime?.path === documentPath ? runtime.state : { status: "loading" });

  const onMessage = useCallback(
    async (e: MessageEvent) => {
      const win = iframeRef.current?.contentWindow;
      if (!win || e.source !== win) return; // only our iframe
      const m = e.data || {};
      const event = pluginUiRuntimeEvent(m.__ctUiState);
      if (event) {
        setRuntime((current) => ({ path: documentPath, state: pluginUiHostTransition(current?.path === documentPath ? current.state : { status: "loading" }, event) }));
        return;
      }
      if (m.__ctReady) {
        win.postMessage({ __ctTheme: themeCss() }, "*");
        win.postMessage({ __ctActive: activeRef.current }, "*");
        return;
      }
      if (m.__ctClose) {
        onClose?.();
        return;
      }
      if (m.__ctInvoke != null) {
        try {
          // iframes (e.g. Git) never need to know their own path.
          const args =
            cwd && m.args && typeof m.args === "object" && (m.args as { cwd?: unknown }).cwd == null
              ? { ...m.args, cwd }
              : m.args;
          const result = await invoke("plugin_view_call", { pluginId, method: m.method, args });
          // In-panel actions invalidate their bubble immediately.
          const usedCwd = (args as { cwd?: unknown } | undefined)?.cwd;
          if (typeof usedCwd === "string") invalidatePluginBubble(pluginId, usedCwd);
          win.postMessage({ __ctReply: m.__ctInvoke, result }, "*");
        } catch (err) {
          win.postMessage({ __ctReply: m.__ctInvoke, error: String(err) }, "*");
        }
      }
    },
    [pluginId, cwd, onClose, documentPath],
  );

  useEffect(() => {
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, [onMessage]);

  useEffect(() => {
    if (!kpiNotices) return;
    let alive = true;
    let unlisten: UnlistenFn | undefined;
    listen(KPI_RECORDED_EVENT, () => {
      if (activeRef.current) iframeRef.current?.contentWindow?.postMessage({ [KPI_CHANGE_NOTICE]: true }, "*");
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
    iframeRef.current?.contentWindow?.postMessage({ __ctActive: active }, "*");
    if (active && focusOnActive && !embedded && state.status === "ready") iframeRef.current?.focus();
  }, [active, focusOnActive, embedded, state.status]);

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
        ref={iframeRef}
        data-testid="extensions-plugin-iframe"
        data-plugin-id={pluginId}
        srcDoc={html}
        title={`${pluginId} view`}
        sandbox="allow-scripts"
        style={{ flex: 1, width: "100%", border: "none", background: "var(--ct-bg)", display: "block", height: "100%" }}
      />}
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
