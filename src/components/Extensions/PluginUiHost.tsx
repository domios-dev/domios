import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useEffect, type ReactNode } from "react";
import type { PluginUiHostState } from "../../lib/pluginUiHostModel";
import { DEV_BUILD } from "../../lib/devBuild";
import { Button } from "../shared/Button";
import { BusyState } from "../shared/BusyState";
import { DeferredLoader } from "../shared/DeferredLoader";

export function PluginUiHost({ pluginId, state, onRetry, children, height = "100%" }: {
  pluginId: string;
  state: PluginUiHostState;
  onRetry: () => void;
  children?: ReactNode;
  height?: number | string;
}) {
  const reason = state.status === "error" || state.status === "empty" ? state.reason : null;
  useEffect(() => {
    if (reason && DEV_BUILD) console.error(`[PLUGIN-UI] ${pluginId}: ${reason}`);
  }, [pluginId, reason]);

  return (
    <div data-plugin-ui-state={state.status} style={{ position: "relative", width: "100%", height, minHeight: 120, overflow: "auto" }}>
      <div aria-hidden={state.status !== "ready"} style={{ height: "100%", visibility: state.status === "ready" ? "visible" : "hidden" }}>
        {children}
      </div>
      {state.status !== "ready" && (
        <div style={{ position: "absolute", inset: 0, display: "flex", alignItems: "center", justifyContent: "center", padding: 16 }}>
          {state.status === "loading" ? (
            <DeferredLoader state="present"><BusyState label="Loading plugin UI" inline /></DeferredLoader>
          ) : (
            <div aria-busy={false} role={state.status === "error" ? "alert" : "status"} style={{ fontSize: 12, overflowWrap: "anywhere" }}>
              <InlineErrorNotice error={reason} operation="opening the plugin interface" />
              {state.status === "empty" && <p>Open the plugin settings or retry after updating the plugin.</p>}
              <Button size="sm" variant="ghost" onClick={onRetry}>Retry plugin UI</Button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
