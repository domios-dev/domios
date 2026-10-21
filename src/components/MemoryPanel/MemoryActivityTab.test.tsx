import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { __setListenForTest } from "../../lib/bridge";
import { setAgentDescriptors } from "../../lib/agentDescriptorCache";
import type { ApiConfig, AppConfig, GraphWorkerInfo } from "../../lib/types";
import type { MemNode } from "./types";
import { MemoryActivityTab, WorkerHistory } from "./MemoryActivityTab";

const worker: GraphWorkerInfo = {
  worker_id: "opaque-worker",
  tab_id: null,
  intent: "extract:opaque-source",
  spawned_by: "auto:save",
  spawned_at: 1000,
  status: "error",
  progress_text: null,
  touched_nodes: ["opaque-source"],
  color: "red",
  report: null,
  done_at: 2000,
  agent_provider: "fixture",
  model: "fixture-model",
  extraction_retry: {
    attempt: 3,
    parked: true,
    retry_after: null,
    reason: "opaque failure evidence",
  },
};
const config = {
  memory: {
    extraction_agent: "fixture",
    extraction_model: "fixture-model",
    dreaming_agent: "fixture",
    dreaming_model: "dream-model",
    auto_extract: true,
    file_sync_enabled: true,
    file_sync_interval_minutes: 60,
  },
  general_agent: { agent_provider: "fixture" },
} as AppConfig;
const api = { host: "localhost", port: 80, token: "fixture" } as ApiConfig;

function control(host: HTMLElement, selector: string, name: string): HTMLElement {
  const element = [...host.querySelectorAll<HTMLElement>(selector)].find((item) =>
    (item.getAttribute("aria-label") || item.textContent || "").includes(name),
  );
  assert.ok(element, `Missing accessible control: ${name}`);
  return element;
}
async function click(element: HTMLElement) {
  await act(async () => element.click());
}

async function main() {
  const window = new Window({ url: "http://localhost/" });
  Object.assign(window, { __CODETERM_DAEMON__: { origin: "http://localhost", token: "fixture" } });
  for (const key of [
    "window",
    "document",
    "HTMLElement",
    "SVGElement",
    "Element",
    "Node",
    "Event",
  ] as const) {
    Object.defineProperty(globalThis, key, {
      value: (window as unknown as Record<string, unknown>)[key],
      configurable: true,
    });
  }
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  __setListenForTest(async () => () => {});
  setAgentDescriptors([]);
  const nodes = new Map([
    ["opaque-source", { id: "opaque-source", title: "A saved decision" } as MemNode],
  ]);
  const requests: string[] = [];
  let failWorkers = false;
  let rows = [worker, { ...worker, worker_id: "older", spawned_at: 500 }];
  globalThis.fetch = (async (input: string | URL | Request, init?: RequestInit) => {
    const path = String(input);
    requests.push(`${init?.method || "GET"} ${path}`);
    if (path.endsWith("/api/mem/workers") && failWorkers) return Response.json({}, { status: 503 });
    if (path.endsWith("/api/mem/workers")) return Response.json(rows);
    if (path.includes("/api/mem/workers/")) return Response.json({});
    if (path.includes("/api/crons")) return Response.json([]);
    if (path.includes("/api/mem/sync/status"))
      return Response.json({ tracked_files: 4, stale_files: 1 });
    if (path.includes("/api/mem/activity/recent"))
      return Response.json({
        events: [
          {
            op: "write",
            ts_ms: 1000,
            mem_ids: ["opaque-source"],
            source: {
              kind: "worker",
              worker_id: "opaque-worker",
              tab_id: null,
              agent_provider: "fixture",
            },
          },
          {
            op: "read",
            ts_ms: 500,
            mem_ids: [],
            source: { kind: "rest", worker_id: null, tab_id: null, agent_provider: null },
          },
        ],
      });
    throw new Error(`Unexpected request ${path}`);
  }) as typeof fetch;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  let navigated = "";
  try {
    await act(async () =>
      root.render(
        <MemoryActivityTab
          api={api}
          config={config}
          nodes={nodes}
          onNavigateToItem={(id) => {
            navigated = id;
          }}
        />,
      ),
    );
    assert.equal(control(host, '[role="tab"]', "Workers").getAttribute("aria-selected"), "true");
    const extraction = control(host, "button", "Extraction");
    assert.equal(extraction.getAttribute("aria-expanded"), "false");
    assert.ok(extraction.textContent?.includes("fixture-model"));
    assert.ok(extraction.textContent?.includes("parked"));
    assert.ok(!host.textContent?.includes("opaque failure evidence"));
    assert.ok(!host.textContent?.includes(worker.intent));
    await click(extraction);
    const history = host.querySelector<HTMLElement>('section[aria-label="extraction details"]');
    assert.ok(history);
    assert.equal(history.querySelectorAll(":scope > details").length, 2);
    const summary = control(history, "summary", "A saved decision");
    await click(summary);
    assert.equal(summary.parentElement?.hasAttribute("open"), true);
    assert.ok(summary.parentElement?.textContent?.includes("opaque failure evidence"));
    await click(control(history, "button", "Open affected memory"));
    assert.equal(navigated, "opaque-source");
    await click(control(host, '[role="tab"]', "Services"));
    assert.equal(host.querySelector('[role="tabpanel"]')?.id, "activity-services-panel");
    await click(control(host, "summary", "Memory operations"));
    await click(control(host, "button", "A saved decision"));
    assert.equal(navigated, "opaque-source");
    const filter = host.querySelector<HTMLSelectElement>(
      'select[aria-label="Filter memory operations"]',
    );
    assert.ok(filter);
    await act(async () => {
      filter.value = "write";
      filter.dispatchEvent(new window.Event("change", { bubbles: true }) as unknown as Event);
    });
    assert.equal(host.querySelectorAll("ol > li").length, 1);
    await click(control(host, '[role="tab"]', "Workers"));
    await click(control(host, "button", "Close details"));
    failWorkers = true;
    await click(control(host, "button", "Refresh activity"));
    const failedLoad = control(host, "summary", "Worker status unavailable");
    assert.ok(failedLoad);
    assert.ok(control(host, "button", "Extraction").textContent?.includes("Status unavailable"));
    failWorkers = false;
    rows = [
      {
        ...worker,
        worker_id: "live",
        status: "analyzing",
        extraction_retry: null,
        done_at: null,
        spawned_at: 3000,
      },
    ];
    await click(control(host, "button", "Refresh activity"));
    await click(control(host, "button", "Extraction"));
    await click(control(host, "summary", "A saved decision"));
    await click(control(host, "button", "Stop worker"));
    assert.ok(
      requests.some(
        (request) => request.startsWith("DELETE ") && request.endsWith("/api/mem/workers/live"),
      ),
    );
    let opened = 0;
    let stopped = 0;
    await act(async () =>
      root.render(
        <WorkerHistory
          worker={{ ...worker, status: "reading" }}
          label="opaque task"
          onOpen={() => {
            opened++;
          }}
          onKill={() => {
            stopped++;
          }}
          onNavigateToItem={() => {}}
        />,
      ),
    );
    await click(control(host, "summary", "opaque task"));
    await click(control(host, "button", "View output"));
    await click(control(host, "button", "Stop worker"));
    assert.equal(opened, 1);
    assert.equal(stopped, 1);
    console.log(
      "✓ Activity tabs, agent history, failure evidence, navigation, filtering, refresh recovery and live controls",
    );
  } finally {
    await act(async () => root.unmount());
    __setListenForTest(null);
    host.remove();
  }
}
void main();
