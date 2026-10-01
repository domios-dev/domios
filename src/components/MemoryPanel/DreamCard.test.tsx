import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { DreamCard, DreamDetailModal, type DreamDiaryEntry } from "./DreamCard";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom(clipboard: string[]) {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  for (const key of ["window", "document", "HTMLElement", "SVGElement", "Element", "Node", "Event"] as const) {
    Object.defineProperty(globalThis, key, { value: (window as never as Record<string, unknown>)[key], configurable: true });
  }
  const gcs = (() => ({ getPropertyValue: () => "" })) as unknown as typeof globalThis.getComputedStyle;
  Object.defineProperty(globalThis, "getComputedStyle", { value: gcs, configurable: true });
  (window as never as Record<string, unknown>).getComputedStyle = gcs;
  Object.defineProperty(globalThis, "navigator", {
    value: {
      userAgent: "node",
      clipboard: { writeText: async (text: string) => { clipboard.push(text); } },
    },
    configurable: true,
  });
}

const LOG_BODY = [
  "<insight>insight-token-91c2</insight>",
  "<log>",
  "DREAM_BACKLINKS: 178",
  "ISLANDS: bridged=0 promoted=1 deleted=0",
  "ORPHANS: bridged=2 deleted=0",
  "</log>",
].join("\n");

function entry(body: string): DreamDiaryEntry {
  return {
    id: "daily_dream_test",
    title: "dream-title-token",
    body,
    created_at: "2026-09-11T08:30:00.000000+00:00",
    area: "_dreams",
    tags: ["dream-diary"],
  };
}

async function renderModal(body: string, report: string | null) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(<DreamDetailModal entry={entry(body)} report={report} onClose={() => {}} />);
  });
  const modal = document.querySelector('[data-testid="memory-dream-modal"]');
  assert(modal, "modal must render into the document");
  return { root, modal };
}

async function dream_modal_has_summary_and_no_log_toggle() {
  const clipboard: string[] = [];
  setupDom(clipboard);

  const fromLog = await renderModal(LOG_BODY, null);
  const modal = fromLog.modal;
  assert(!modal.querySelector('[data-testid="memory-dream-log-toggle"]'), "the log toggle is gone");
  assert(!modal.querySelector('[data-testid="memory-dream-log"]'), "the collapsible log is gone");
  const summary = modal.querySelector('[data-testid="memory-dream-summary"]');
  assert(summary, "a log with counters must render a summary");
  assert(summary.getAttribute("data-summary-source") === "log", "summary must come from the log");
  const text = summary.textContent ?? "";
  for (const value of ["178", "1", "2"]) {
    assert(new RegExp(`\\b${value}\\b`).test(text), `summary must carry counter ${value}: ${text}`);
  }
  const section = summary.closest("section");
  const labelId = section?.getAttribute("aria-labelledby") ?? "";
  assert(labelId && document.getElementById(labelId)?.textContent, "the summary must carry a label");

  assert(modal.getAttribute("role") === "dialog", "modal keeps its dialog role");
  assert(modal.getAttribute("aria-label") === "dream-title-token", "modal is named by the dream title");
  for (const id of ["memory-dream-copy-insight", "memory-dream-modal-close", "memory-dream-copy-raw"]) {
    const button = modal.querySelector(`[data-testid="${id}"]`);
    assert(button?.getAttribute("aria-label"), `${id} must keep an accessible name`);
  }

  const raw = modal.querySelector<HTMLElement>('[data-testid="memory-dream-copy-raw"]');
  assert(raw, "copy-raw stays available");
  await act(async () => {
    raw.click();
    await Promise.resolve();
  });
  assert(clipboard[clipboard.length - 1] === LOG_BODY, "copy-raw must hand over the full stored body, log included");
  await act(async () => fromLog.root.unmount());

  const fromReport = await renderModal(LOG_BODY, "report-token-5d0e");
  const reported = fromReport.modal.querySelector('[data-testid="memory-dream-summary"]');
  assert(reported, "a run report must render a summary");
  assert(reported.getAttribute("data-summary-source") === "report", "the run report must win");
  assert(reported.textContent === "report-token-5d0e", `summary must be the report: ${reported.textContent}`);
  await act(async () => fromReport.root.unmount());

  const bare = await renderModal("<insight>insight only</insight>", null);
  assert(!bare.modal.querySelector('[data-testid="memory-dream-summary"]'), "no data renders no summary");
  await act(async () => bare.root.unmount());
  console.log("✓ dream_modal_has_summary_and_no_log_toggle");
}

async function dream_card_centers_text() {
  setupDom([]);
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const longInsight = "x".repeat(400);
  await act(async () => {
    root.render(
      <DreamCard
        entry={entry(`<insight>${longInsight}</insight>`)}
        recordState={{ state: "incomplete_recoverable", reason: "reason-token" }}
        onOpen={() => {}}
      />,
    );
  });
  const body = host.querySelector('[data-testid="memory-dream-card-body"]');
  assert(body, "card body must render");
  const classes = body.className.split(/\s+/);
  for (const cls of ["flex", "items-center", "justify-center", "text-center"]) {
    assert(classes.includes(cls), `card body must center its text (${cls}): ${body.className}`);
  }
  const clamp = body.querySelector(".line-clamp-6");
  assert(clamp, "the excerpt keeps its line clamp");
  assert((clamp.textContent ?? "").length < longInsight.length, "the excerpt stays truncated");
  const badge = host.querySelector('[data-testid="memory-dream-card-incomplete"]');
  assert(badge, "incomplete badge renders");
  assert(badge.getAttribute("data-dream-record-state") === "incomplete_recoverable", "badge keeps its state");
  assert(badge.parentElement?.className.split(/\s+/).includes("justify-center"), "the badge row is centered");
  await act(async () => root.unmount());
  console.log("✓ dream_card_centers_text");
}

void dream_modal_has_summary_and_no_log_toggle().then(dream_card_centers_text);
