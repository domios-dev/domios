import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { __setListenForTest } from "../../lib/bridge";
import type { ApiConfig } from "../../lib/types";
import { MemoryDreamsTab } from "./MemoryDreamsTab";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  for (const key of ["window", "document", "HTMLElement", "SVGElement", "Element", "Node", "Event"] as const) {
    Object.defineProperty(globalThis, key, { value: (window as never as Record<string, unknown>)[key], configurable: true });
  }
  const gcs = (() => ({ getPropertyValue: () => "" })) as unknown as typeof globalThis.getComputedStyle;
  Object.defineProperty(globalThis, "getComputedStyle", { value: gcs, configurable: true });
  (window as never as Record<string, unknown>).getComputedStyle = gcs;
}

const SEARCH_RESPONSE = {
  hits: [
    {
      item: {
        id: "daily_dream_2026_08_05_06_00",
        uid: "u1",
        kind: "daily",
        scope: "global",
        title: "dream: 2026-08-05 06:00",
        summary: null,
        area: null,
        tags: ["dream-diary"],
        body: "<insight>\nTest dream insight for UI repro.\n</insight>",
        source: "mem",
        importance: 0.5,
        created_at: "2026-08-05T06:00:00.000000+00:00",
        updated_at: "2026-08-05T06:00:00.000000+00:00",
        accessed_at: null,
        access_count: 0,
        recurrence_count: 0,
      },
      score: 0.9,
    },
  ],
};

const CRONS_RESPONSE = [
  {
    id: "cid",
    name: "mem-dreaming-auto",
    description: null,
    workspace_id: "",
    schedule: "0 6 * * *",
    enabled: true,
    agent: "claude",
    model: null,
    task: "",
    cwd: null,
    role: null,
    oneshot: false,
    max_history: 5,
    created_at: 0,
    last_run: null,
    next_run: 1785910139624,
    jitter_max_ms: null,
  },
];

async function main() {
  setupDom();
  __setListenForTest(async () => () => {});

  const seen: string[] = [];
  globalThis.fetch = (async (url: string | URL) => {
    const u = String(url);
    seen.push(u);
    if (u.includes("/api/mem/search")) {
      return { ok: true, json: async () => SEARCH_RESPONSE } as Response;
    }
    if (u.includes("/api/crons")) {
      return { ok: true, json: async () => CRONS_RESPONSE } as Response;
    }
    if (u.includes("/api/mem/workers")) {
      return { ok: true, json: async () => [] } as Response;
    }
    return { ok: true, json: async () => ({}) } as Response;
  }) as typeof fetch;

  const api: ApiConfig = { enabled: true, host: "127.0.0.1", port: 7686, token: "t", cors: false, cors_allow_origins: [] };
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);

  await act(async () => {
    root.render(<MemoryDreamsTab api={api} />);
  });
  await act(async () => { await Promise.resolve(); await Promise.resolve(); });

  const searchUrl = seen.find((u) => u.includes("/api/mem/search")) ?? "";
  console.log("search url:", searchUrl);
  const cards = host.querySelectorAll('[data-testid="memory-dream-card"]');
  const empty = host.querySelector('[data-testid="memory-dream-empty"]');
  console.log("cards:", cards.length, "empty shown:", !!empty);
  console.log("header status:", host.querySelector('[data-testid="memory-dreams-tab"]')?.textContent?.slice(0, 120));

  assert(cards.length === 1, `expected 1 dream card, got ${cards.length}`);
  assert(!empty, "empty state should not render when a dream exists");

  await act(async () => root.unmount());
  __setListenForTest(null);
  console.log("✓ DreamsTab renders a seeded dream as a card");
}

void main();
