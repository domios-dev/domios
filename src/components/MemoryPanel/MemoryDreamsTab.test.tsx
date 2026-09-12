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

const DIARY_RESPONSE = {
  items: [
    {
      id: "daily_dream_2026_08_05_06_00",
      uid: "u1",
      kind: "daily",
      scope: "global",
      title: "dream: 2026-08-05 06:00",
      summary: null,
      area: "_dreams",
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
  ],
  total: 31,
  next_offset: 30,
};

// One complete diary and one still holding the REM skeleton, on a page that is
// smaller than the store's total.
const MIXED_DIARY_RESPONSE = {
  items: [
    {
      ...DIARY_RESPONSE.items[0],
      id: "daily_dream_2026_09_11_08_30",
      title: "dream: 2026-09-11 08:30",
      body: [
        "<insight>WIP: dreaming...</insight>",
        "<log>",
        "PATTERN: pending",
        "CALIBRATION: pending",
        "</log>",
      ].join("\n"),
      created_at: "2026-09-11T08:30:00.000000+00:00",
    },
    DIARY_RESPONSE.items[0],
  ],
  total: 31,
  next_offset: 30,
};

const UNRELATED_SEARCH_RESPONSE = {
  hits: [
    {
      item: {
        id: "unrelated",
        kind: "fact",
        title: "unrelated",
        body: "unrelated",
      },
      score: 1,
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
    if (u.includes("/api/mem/dream/diary")) {
      return { ok: true, json: async () => DIARY_RESPONSE } as Response;
    }
    if (u.includes("/api/mem/search")) {
      return { ok: true, json: async () => UNRELATED_SEARCH_RESPONSE } as Response;
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

  const diaryUrl = seen.find((u) => u.includes("/api/mem/dream/diary")) ?? "";
  console.log("diary url:", diaryUrl);
  const cards = host.querySelectorAll('[data-testid="memory-dream-card"]');
  const empty = host.querySelector('[data-testid="memory-dream-empty"]');
  console.log("cards:", cards.length, "empty shown:", !!empty);
  console.log("header status:", host.querySelector('[data-testid="memory-dreams-tab"]')?.textContent?.slice(0, 120));

  assert(cards.length === 1, `expected 1 dream card, got ${cards.length}`);
  assert(!empty, "empty state should not render when a dream exists");
  assert(diaryUrl.includes("limit=30"), `expected bounded diary page, got ${diaryUrl}`);
  assert(!diaryUrl.includes("q=dream"), "diary route must not use relevance search");
  assert(!seen.some((u) => u.includes("/api/mem/search")), "must not request relevance search");
  assert(
    host.querySelector('[data-testid="memory-dreams-tab"]')?.textContent?.includes("31 dreams recorded"),
    "header must display the full diary total",
  );

  await act(async () => root.unmount());
  __setListenForTest(null);
  console.log("✓ memory_dreams_tab_uses_diary_route_and_renders_newest_first");
}

// The store total and the page's incompleteness come from different
// populations, so the header may carry only one of them as a number.
async function incompleteOnPageKeepsOneCount() {
  setupDom();
  __setListenForTest(async () => () => {});

  globalThis.fetch = (async (url: string | URL) => {
    const u = String(url);
    if (u.includes("/api/mem/dream/diary")) {
      return { ok: true, json: async () => MIXED_DIARY_RESPONSE } as Response;
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

  const header = host.querySelector('[data-testid="memory-dream-meta"]')?.textContent ?? "";
  console.log("header with an unfinished diary in view:", header.slice(0, 160));

  assert(header.includes("31 dreams recorded"), `header must keep the store total: ${header}`);
  assert(
    !!host.querySelector('[data-testid="memory-dream-unfinished-flag"]'),
    "an unfinished diary in view must be flagged",
  );
  const secondCount = header.match(/\d+\s*(incomplete|unfinished|pending|of)\b/i);
  assert(
    secondCount === null,
    `the page must not print a second diary count: ${secondCount?.[0]}`,
  );
  assert(
    !/\bof 31\b/.test(header),
    `the flag must not restate the store total: ${header}`,
  );
  const badges = host.querySelectorAll('[data-testid="memory-dream-card-incomplete"]');
  assert(badges.length === 1, `expected 1 incomplete card badge, got ${badges.length}`);
  assert(
    badges[0].getAttribute("data-dream-record-state") === "incomplete_recoverable",
    "a placeholder diary with no live run is recoverable history, not live work",
  );

  await act(async () => root.unmount());
  __setListenForTest(null);
  console.log("✓ a page-scoped incompleteness flag never adds a second count");
}

void main().then(incompleteOnPageKeepsOneCount);
