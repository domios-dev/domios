/**
 * Host for a plugin's iframe UI (the `view` capability). Renders the plugin's
 * `ui/index.html` in a sandboxed iframe (origin-null, CSP-locked) and relays the
 * `ct.invoke` postMessage bridge to the permission-gated `plugin_view_call`.
 *
 * The iframe holds no capability: its only channel is postMessage to us, and we
 * forward each call to the host (which stamps the plugin id + checks permissions).
 */
import { useCallback, useEffect, useRef } from "react";
import { invoke } from "../../lib/bridge";
import { daemonHttpBase, daemonAuthToken, type DaemonApiConfig } from "../../lib/webApi";

type ApiCfg = DaemonApiConfig;

// Theme tokens pushed into the iframe so plugin UIs match the app.
const THEME_VARS = [
  "--ct-bg",
  "--ct-bg-elev",
  "--ct-fg",
  "--ct-muted",
  "--ct-accent",
  "--ct-border-default",
  "--ct-border-subtle",
  "--ct-ok",
  "--ct-warn",
  "--ct-err",
];

function themeCss(): string {
  const cs = getComputedStyle(document.documentElement);
  return THEME_VARS.map((v) => `${v}:${cs.getPropertyValue(v).trim()}`)
    .filter((s) => !s.endsWith(":"))
    .join(";");
}

export function PluginView({
  pluginId,
  api,
  onClose,
  embedded,
  cwd,
}: {
  pluginId: string;
  api: ApiCfg | undefined;
  onClose?: () => void;
  /** Render just the iframe (no fullscreen chrome) to embed in a detail panel. */
  embedded?: boolean;
  /** Pane cwd stamped into view-call args that omit one (e.g. the Git panel). */
  cwd?: string | null;
}) {
  const iframeRef = useRef<HTMLIFrameElement>(null);

  const src = `${daemonHttpBase(api)}/api/plugins/${encodeURIComponent(pluginId)}/ui/index.html?token=${encodeURIComponent(daemonAuthToken(api))}`;

  const onMessage = useCallback(
    async (e: MessageEvent) => {
      const win = iframeRef.current?.contentWindow;
      if (!win || e.source !== win) return; // only our iframe
      const m = e.data || {};
      if (m.__ctReady) {
        win.postMessage({ __ctTheme: themeCss() }, "*");
        return;
      }
      if (m.__ctInvoke != null) {
        try {
          // Stamp the pane cwd into args that don't carry one, so confined
          // iframes (e.g. Git) never need to know their own path.
          const args =
            cwd && m.args && typeof m.args === "object" && (m.args as { cwd?: unknown }).cwd == null
              ? { ...m.args, cwd }
              : m.args;
          const result = await invoke("plugin_view_call", { pluginId, method: m.method, args });
          win.postMessage({ __ctReply: m.__ctInvoke, result }, "*");
        } catch (err) {
          win.postMessage({ __ctReply: m.__ctInvoke, error: String(err) }, "*");
        }
      }
    },
    [pluginId, cwd],
  );

  useEffect(() => {
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, [onMessage]);

  const frame = (
    <iframe
      ref={iframeRef}
      src={src}
      title={`${pluginId} view`}
      sandbox="allow-scripts"
      style={{ flex: 1, width: "100%", border: "none", background: "var(--ct-bg)", display: "block" }}
    />
  );

  if (embedded) {
    return (
      <div style={{ width: "100%", height: "100%", display: "flex", borderRadius: 8, overflow: "hidden", border: "1px solid var(--ct-border-subtle, rgba(255,255,255,0.08))" }}>
        {frame}
      </div>
    );
  }

  return (
    <div style={{ position: "absolute", inset: 0, display: "flex", flexDirection: "column", background: "var(--ct-bg)" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "8px 12px", borderBottom: "1px solid var(--ct-border-subtle, rgba(255,255,255,0.06))" }}>
        <button
          onClick={onClose}
          style={{ background: "transparent", border: "none", color: "var(--ct-muted)", cursor: "pointer", fontSize: 12.5 }}
        >
          ← Back
        </button>
        <span style={{ fontSize: 12.5, color: "var(--ct-muted)" }}>{pluginId}</span>
      </div>
      {frame}
    </div>
  );
}
