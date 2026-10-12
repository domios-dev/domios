import { createCommandRegistry, dispatchCommand, CANNED_COMMANDS, REFUSED_COMMANDS } from './transportCommands';
import { nativeCommand, NATIVE_COMMANDS } from './nativeCommands';
import { ensureDaemonRuntime, selectedDaemonTransport } from './daemonRuntime';
import { DEV_BUILD } from "./devBuild";

import {
  currentToken,
  IS_WEB,
  WebApiError,
} from "./webApi";
import { webListen } from "./webEvents";
import { WEB_UNSUPPORTED } from "./bridgeWebUnsupported";
import { ipcLogArgs } from "./ipcLogArgs";
import { checkBuildMismatch } from "./buildMismatchGuard";
import type { TabInfo } from "./types";
import { classifyTabTarget, hasCompositeTabArgument, remoteTransportCall } from "./remoteTabTransport";
import { subscribeRemoteTabEvent } from "./remoteTabEvents";

export type UnlistenFn = () => void;
export { IS_WEB };

export interface ProviderIntegrationHealth {
  integration_state?: "absent" | "stale_version" | "untrusted_hash" | "installed" | string;
  repair_requires_consent?: boolean;
  hooks_installed?: boolean;
  stop_hook?: boolean;
  session_start_hook?: boolean;
  codeterm_binding_fallback_source?: string;
  plugin_installed?: boolean;
  plugin_file_current?: boolean;
  status?: string;
  target?: string;
  [key: string]: unknown;
}

type Args = Record<string, unknown>;
const webPtyOffsets = new Map<string, number>();
const webPtyTruncations = new Map<string, number>();
let commandRegistry: ReturnType<typeof createCommandRegistry> | undefined;
function commands(): ReturnType<typeof createCommandRegistry> {
  return commandRegistry ??= createCommandRegistry(selectedDaemonTransport, { offsets: webPtyOffsets, truncations: webPtyTruncations });
}
let invokeForTest: (<T>(cmd: string, args?: Args) => Promise<T>) | null = null;
let listenForTest:
  | (<T>(event: string, handler: (e: { payload: T }) => void) => Promise<UnlistenFn>)
  | null = null;

export function __setInvokeForTest(fn: (<T>(cmd: string, args?: Args) => Promise<T>) | null): void {
  invokeForTest = fn;
}

export function __setListenForTest(
  fn: (<T>(event: string, handler: (e: { payload: T }) => void) => Promise<UnlistenFn>) | null,
): void {
  listenForTest = fn;
}

/** Drop the tail cursor so the next read_pty replays retained history from 0. */
export function resetWebPtyTail(ptyId: string): void {
  webPtyOffsets.delete(ptyId);
  webPtyTruncations.delete(ptyId);
}

/** Consume (once) the under-retention marker from the last `read_pty` response,
 * if the ring evicted bytes ahead of the client's cursor. */
export function takePtyTruncation(ptyId: string): number | null {
  const from = webPtyTruncations.get(ptyId);
  if (from === undefined) return null;
  webPtyTruncations.delete(ptyId);
  return from;
}

/** True iff a `read_pty` failure means the PTY is confirmed gone — a stable,
 * structured signal `usePtyConnection` can trust to respawn, as opposed to a
 * transient transport hiccup that should just retry. */
export function isPtyDeadError(err: unknown): boolean {
  if (err instanceof WebApiError) return err.status === 404;
  const message = err instanceof Error ? err.message : String(err);
  return message.includes("PTY not found") || message.includes("PTY closed");
}

export class WebUnsupportedError extends Error {
  readonly code = "WEB_UNSUPPORTED" as const;

  constructor(readonly command: string) {
    super(`[web] command not supported in browser mode: ${command}`);
    this.name = "WebUnsupportedError";
  }
}

export type WebCapability =
  | { supported: true }
  | { supported: false; reason: string };

/** Whether a Tauri IPC command is reachable from the web mirror. */
export function webCapability(cmd: string): WebCapability {
  if (Object.prototype.hasOwnProperty.call(commands(), cmd) || Object.prototype.hasOwnProperty.call(CANNED_COMMANDS, cmd)) return { supported: true };
  if (cmd === "open_url" || cmd === "read_clipboard_text") return { supported: true };
  if (Object.prototype.hasOwnProperty.call(NATIVE_COMMANDS, cmd)) return IS_WEB
    ? { supported: false, reason: WEB_UNSUPPORTED[cmd] ?? "Native shell capability required" }
    : { supported: true };
  return { supported: false, reason: REFUSED_COMMANDS[cmd as keyof typeof REFUSED_COMMANDS] ?? "unmapped command: " + cmd };
}

export function isWebUnsupportedError(err: unknown): err is WebUnsupportedError {
  return err instanceof WebUnsupportedError;
}

async function invokeRaw<T>(cmd: string, args: Args = {}): Promise<T> {
  if (Object.prototype.hasOwnProperty.call(commands(), cmd)) {
    ensureDaemonRuntime();
    return dispatchCommand<T>(commands(), cmd, args);
  }
  const native = nativeCommand(cmd, args);
  if (native) return await native as T;
  if (Object.prototype.hasOwnProperty.call(CANNED_COMMANDS, cmd)) return CANNED_COMMANDS[cmd as keyof typeof CANNED_COMMANDS] as T;
  ensureDaemonRuntime();
  return dispatchCommand<T>(commands(), cmd, args);
}

const IPC_SLOW_MS = 300;
type IpcSample = { cmd: string; ms: number; at: number };
type IpcPerf = {
  invokes: number;
  invokeMs: number;
  events: number;
  byCommand: Record<string, number>;
  byEvent: Record<string, number>;
};

declare global {
  interface Window {
    __ipcPerf?: IpcPerf;
  }
}

// Diagnostic: report any main-thread block (render / term.write / sync JS) >300ms.
if (typeof PerformanceObserver !== "undefined") {
  try {
    new PerformanceObserver((list) => {
      for (const e of list.getEntries()) {
        if (e.duration >= IPC_SLOW_MS) {
          // eslint-disable-next-line no-console
          console.warn(`[LONGTASK] main thread blocked ${e.duration.toFixed(0)}ms`);
          const w = window as unknown as { __ctPerf?: IpcSample[] };
          (w.__ctPerf ??= []).push({ cmd: "LONGTASK", ms: e.duration, at: Date.now() });
        }
      }
    }).observe({ entryTypes: ["longtask"] });
  } catch { /* longtask unsupported */ }
}

export async function invoke<T>(cmd: string, args?: Args): Promise<T> {
  const t0 = performance.now();
  try {
    const target = classifyTabTarget(args?.tabId ?? args?.tab_id ?? args?.ptyId ?? args?.pty_id);
    if (target.kind === "invalid-remote") {
      throw new Error(`Invalid remote tab id "${target.value}"; local fallback is forbidden`);
    }
    if (invokeForTest) return await invokeForTest<T>(cmd, args);
    const remote = remoteTransportCall(cmd, args);
    if (remote) {
      const value = await invokeRaw<unknown>("mesh_remote_tab_action", {
        ordinaryTabId: remote.ordinaryTabId,
        peerId: remote.peerId,
        method: remote.method,
        suffix: remote.suffix,
        body: remote.body,
      });
      return (remote.select ? remote.select(value) : value) as T;
    }
    if (hasCompositeTabArgument(args)) {
      throw new Error(`Remote tab action "${cmd}" is unavailable; local fallback is forbidden`);
    }
    return await invokeRaw<T>(cmd, args);
  } catch (err) {
    if (isCommandNotFoundError(err)) {
      checkBuildMismatch().catch(() => {});
    }
    if (cmd === "read_workspace_essence_maps") {
      if (DEV_BUILD) console.warn("Workspace map request failed", err);
      throw new Error("Could not load project maps. Try again.");
    }
    throw err;
  } finally {
    const perf = typeof window !== "undefined" ? window.__ipcPerf : undefined;
    if (perf) {
      perf.invokes += 1;
      perf.invokeMs += performance.now() - t0;
      perf.byCommand[cmd] = (perf.byCommand[cmd] ?? 0) + 1;
    }
    if (!invokeForTest && typeof window !== "undefined") {
      const ms = performance.now() - t0;
      if (ms >= IPC_SLOW_MS) {
        // eslint-disable-next-line no-console
        console.warn(`[SLOW-IPC] ${cmd} ${ms.toFixed(0)}ms`, ipcLogArgs(args));
        const w = window as unknown as { __ctPerf?: IpcSample[] };
        (w.__ctPerf ??= []).push({ cmd, ms, at: Date.now() });
        if (w.__ctPerf.length > 200) w.__ctPerf.shift();
      }
    }
  }
}

export function checkProviderIntegration(providerId: string): Promise<ProviderIntegrationHealth> {
  return invoke<ProviderIntegrationHealth>("check_provider_integration", { provider_id: providerId });
}

export function installProviderIntegration(
  providerId: string,
): Promise<void> {
  return invoke<void>("install_provider_integration", {
    provider_id: providerId,
    consent: true,
  });
}

export function uninstallProviderIntegration(providerId: string): Promise<void> {
  return invoke<void>("uninstall_provider_integration", { provider_id: providerId });
}

function isCommandNotFoundError(err: unknown): boolean {
  const msg = String(err);
  return /command.*not found/i.test(msg);
}

/** Reopen a recently-closed tab in a workspace. `index` selects the ring
 *  entry (0 = most recent, the default). Works in both Tauri and web modes. */
export function reopenClosedTab(workspaceId: string, index = 0): Promise<TabInfo> {
  return invoke<TabInfo>("mux_reopen_closed_tab", { workspaceId, index });
}

/** Reopen a whole closed group (single / cascade subtree / workspace) by id.
 *  Recreates the workspace if needed and restores every tab, relinking the
 *  orchestration hierarchy. Returns workspace + tab ids of the activated tab. */
export function reopenClosedGroup(groupId: string): Promise<{ workspace_id: string; tab_id: string }> {
  return invoke("mux_reopen_closed_group", { groupId });
}

/** Reopen a tab from the orphaned ring (its workspace was deleted). Recreates
 *  the workspace automatically. Returns workspace + tab ids. */
export function restoreOrphanedTab(tabId: string): Promise<{ workspace_id: string; tab_id: string }> {
  return invoke("mux_restore_orphaned_tab", { tabId });
}

export async function listen<T>(
  event: string,
  handler: (e: { payload: T }) => void,
): Promise<UnlistenFn> {
  const counted = (e: { payload: T }) => {
    const perf = typeof window !== "undefined" ? window.__ipcPerf : undefined;
    if (perf) {
      perf.events += 1;
      perf.byEvent[event] = (perf.byEvent[event] ?? 0) + 1;
    }
    if (event === "config-changed" && e.payload && typeof e.payload === "object") {
      const config = e.payload as Record<string, unknown>;
      const api = config.api as Record<string, unknown> | undefined;
      if (api) api.token = currentToken();
    }
    handler(e);
  };
  const remote = subscribeRemoteTabEvent(event, counted);
  if (remote) return remote;
  if (listenForTest) return listenForTest(event, counted);
  ensureDaemonRuntime();
  return webListen(event, counted as (e: { payload: unknown }) => void);
}

export function localListenBranch(isWeb: boolean): "tauri" | "web" {
  void isWeb;
  return "web";
}
