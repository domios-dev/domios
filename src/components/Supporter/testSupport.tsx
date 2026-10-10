import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { vi } from "vitest";
import { __setListenForTest } from "../../lib/bridge";
import type { ApiConfig } from "../../lib/types";
import type { SupporterStatus } from "../Settings/useDecisionToken";
import { SUPPORTER_JOURNAL_EVENT } from "./useSupporterResource";

export const API: ApiConfig = { enabled: true, host: "127.0.0.1", port: 7686, token: "fixture", cors: false };
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

export function statusFixture(): SupporterStatus {
  return {
    state: "active", adapter: "fixture", connected: true, token_mode: "none", model: null, deadline_ms: 3000,
    token: { adapter: "fixture", token_mode: "none", present: false }, adapter_registered: true, adapters: [], connections: [],
    last_activity: 1700000000, calls_last_hour: 4, calls_last_day: 12, last_verdict: {},
    budget: { reached: false, calls_per_hour: null, free_at: null, storage_ready: true },
    pricing: { display_name: "Jev", input_usd_per_million: 0.5, output_usd_per_million: 0.5 },
    spend: {
      today: { input_tokens: 10, output_tokens: 5, estimated_usd: 0.125 },
      month: { input_tokens: 100, output_tokens: 50, estimated_usd: 0.75 },
      exhausted: false, next_probe_at: null, storage_ready: true,
    },
  };
}

export function harness() {
  let root: Root | undefined;
  let host: HTMLElement | undefined;
  const listeners = new Set<(event: { payload: unknown }) => void>();
  __setListenForTest(async (name, callback) => {
    if (name === SUPPORTER_JOURNAL_EVENT) listeners.add(callback as (event: { payload: unknown }) => void);
    return () => { listeners.delete(callback as (event: { payload: unknown }) => void); };
  });
  return {
    listeners,
    async render(node: ReactNode): Promise<HTMLElement> {
      const mountHost = document.createElement("div");
      host = mountHost;
      document.body.append(mountHost);
      root = createRoot(mountHost);
      await act(async () => { root!.render(node); });
      return mountHost;
    },
    async rerender(node: ReactNode) {
      await act(async () => { root!.render(node); });
    },
    async emit() {
      await act(async () => { for (const callback of listeners) callback({ payload: {} }); });
    },
    async cleanup() {
      if (root) await act(async () => root!.unmount());
      root = undefined;
      host?.remove();
      __setListenForTest(null);
      vi.restoreAllMocks();
    },
  };
}
