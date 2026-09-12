import type { Tab } from "../../lib/types";
import { BusyState } from "../shared/BusyState";

export type TabBootOverlayView =
  | { kind: "none" }
  | { kind: "starting"; label: string }
  | { kind: "failed"; reason: string | null; canRetry: boolean };

export function tabBootOverlayView(tab: Tab | null | undefined): TabBootOverlayView {
  switch (tab?.boot_stage) {
    case "spawn_failed":
      return {
        kind: "failed",
        reason: tab.boot_failure_reason ?? null,
        canRetry: tab.protected === true,
      };
    case "pty_starting":
    case "pty_spawned":
    case "reader_ready":
    case "shell_ready":
    case "agent_launching":
    case "launch_written":
    case "provider_ready":
    // A deadline is provisional: the launch may still prove itself, so the
    // overlay keeps waiting instead of offering a retry over a live agent.
    case "launch_deadline_elapsed":
      return { kind: "starting", label: "Starting" };
    case "agent_ready":
    default:
      return { kind: "none" };
  }
}

interface TabBootOverlayProps {
  view: TabBootOverlayView;
  shellReady: boolean;
  overlayVisible: boolean;
  onRetry: () => void;
}

export function TabBootOverlay({
  view,
  shellReady,
  overlayVisible,
  onRetry,
}: TabBootOverlayProps) {
  if (!overlayVisible && view.kind === "none") return null;

  return (
    <div
      className={`absolute inset-0 ${!shellReady && view.kind !== "failed" ? "terminal-loading-overlay" : ""}`}
      data-testid="terminal-loading-overlay"
      style={{
        background: "var(--ct-bg, #0d1117)",
        opacity: view.kind === "none" && shellReady ? 0 : 1,
        transition: "opacity 0.3s ease-out",
      }}
    >
      {view.kind === "failed" ? (
        <div
          className="absolute bottom-6 left-1/2 max-w-[min(34rem,calc(100%-2rem))] -translate-x-1/2 rounded-md border px-3 py-2 text-center"
          data-testid="tab-boot-failed"
          style={{
            borderColor: "color-mix(in srgb, var(--ct-red, #f85149) 55%, transparent)",
            background: "color-mix(in srgb, var(--ct-bg, #0d1117) 94%, var(--ct-red, #f85149) 6%)",
            color: "var(--ct-fg, #c9d1d9)",
          }}
        >
          <div className="text-sm font-semibold">Launch failed</div>
          {view.reason && (
            <div className="mt-1 truncate text-xs opacity-75" title={view.reason}>
              {view.reason}
            </div>
          )}
          {view.canRetry && (
            <button
              type="button"
              className="mt-2 rounded-md border px-2 py-1 text-xs font-medium"
              aria-label="Retry agent launch"
              title="Retry agent launch"
              onMouseDown={(e) => e.preventDefault()}
              onClick={onRetry}
            >
              Retry
            </button>
          )}
        </div>
      ) : (
        view.kind === "starting" && (
          <div className="absolute bottom-6 left-1/2 -translate-x-1/2">
            <BusyState
              labelKey="tabBoot"
              label={view.label}
              phase="work"
              data-testid="tab-boot-busy"
              style={{ color: "var(--ct-muted-fg, #8b949e)", fontSize: 12 }}
            />
          </div>
        )
      )}
    </div>
  );
}
