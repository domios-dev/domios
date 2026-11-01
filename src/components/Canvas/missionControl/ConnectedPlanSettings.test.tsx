import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { GuideNavigationProvider } from "@codeterm/chat-ui";
import { publishNotice } from "../../../lib/notices";
import { usePlanControlLoop } from "./usePlanControlLoop";
import { daemonFetch } from "../../../lib/webApi";
import { FIXTURE_MILESTONE, FIXTURE_PLAN } from "../cards/fixtures.testdata";
import type { PlanControlLoop } from "../planControlLoop";
import { GUIDE_SECTIONS } from "../../../help/guideNavigation";
import { getHelpContent } from "../../../help/content";
import { ConnectedPlanSettings } from "./ConnectedPlanSettings";
import { PlanRecentActivityRail } from "../planView/PlanRecentActivityRail";
import { planOversight } from "./planOversightAdapter";
import { CronRunHistory } from "../../MeshPanel/CronDrawerSection/CronRunHistory";

vi.mock("../../../lib/notices", () => ({ publishNotice: vi.fn() }));

vi.mock("../../../lib/webApi", async (importOriginal) => ({
  ...await importOriginal<typeof import("../../../lib/webApi")>(),
  daemonFetch: vi.fn(),
}));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const NOW = 1_700_000_000_000;
const API = { enabled: true, host: "localhost", port: 7686, token: "fixture", cors: false };
const BASE: PlanControlLoop = {
  mode: "curator", heartbeat_interval_secs: null, next_at: NOW + 45 * 60_000,
  last_round: null, fallback_reason: null, suspended_reason: null,
  counts_7d: { rounds: 0, quiet: 0, nudged: 0, learnings: 0, saves: 0, fallbacks: 0 },
};
let stored: PlanControlLoop;
let root: Root;
let host: HTMLDivElement;
const requests: Array<{ path: string; init?: RequestInit }> = [];

beforeEach(() => {
  localStorage.clear();
  stored = { ...BASE };
  requests.length = 0;
  vi.mocked(publishNotice).mockClear();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  vi.mocked(daemonFetch).mockImplementation(async (_api, path, init) => {
    requests.push({ path, init });
    if (path.endsWith("/control-loop") && init?.method === "POST") stored = { ...stored, ...JSON.parse(String(init.body)) };
    return Response.json(stored);
  });
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});
async function render(element: ReactElement) { await act(async () => root.render(element)); }
function button(label: string, parent: ParentNode = host) {
  const match = Array.from(parent.querySelectorAll<HTMLButtonElement>("button")).find(node => node.textContent?.trim() === label || node.getAttribute("aria-label") === label || node.getAttribute("aria-label")?.startsWith(`${label}: `));
  if (!match) throw new Error(`Missing button ${label}`);
  return match;
}
async function click(label: string, parent?: ParentNode) {
  await act(async () => {
    const target = button(label, parent);
    if (label === "Heartbeat interval") target.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    else target.click();
  });
}
function panel(plan = FIXTURE_PLAN) { return <ConnectedPlanSettings plan={plan} api={API} now={NOW} />; }

it("requests a round through the owner-confirmed endpoint and disables it while suspended", async () => {
  await render(panel());
  await click("Run now");
  const run = requests.find(({ path }) => path.endsWith("/control-loop/run"));
  expect(run?.init?.method).toBe("POST");
  expect(new Headers(run?.init?.headers).get("X-CodeTerm-Plan-Confirmation")).toBe(FIXTURE_PLAN.id);
  await render(<ConnectedPlanSettings plan={FIXTURE_PLAN} initial={{ ...BASE, suspended_reason: "waiting_on_human" }} api={API} now={NOW} />);
  expect(button("Run now").disabled).toBe(true);
  const pending = vi.fn();
  window.addEventListener("codeterm:open-notification-center", pending, { once: true });
  await click("Open pending questions");
  expect(pending).toHaveBeenCalledTimes(1);
});

it("shows the authoritative Curator outcome in Jobs history", async () => {
  await render(<CronRunHistory allowed runLabel="Curator round" runs={[{ run_id: "round-one", started_at: NOW, finished_at: NOW + 1000, status: "Failed", summary: "failed", tab_id: "closed-tab", session_id: "retained-session" }]} onOpen={() => undefined} />);
  expect(host.textContent).toContain("Curator round · failed");
  expect(host.textContent).not.toContain("Success");
});
it("shows an unavailable Curator refusal beside Run now", async () => {
  await render(panel());
  vi.mocked(daemonFetch).mockResolvedValueOnce(new Response(JSON.stringify({ code: "curator_unavailable", message: "No Curator adapter is connected" }), { status: 400 }));
  await click("Run now");
  expect(host.querySelector('[role="alert"]')?.textContent).toContain("No Curator adapter is connected");
  expect(host.querySelector('[role="alert"]')?.textContent).toContain("No Curator adapter is connected");
  expect(button("Run now").disabled).toBe(false);
});

it("reads persisted Curator settings, switches mode, and persists interval presets", async () => {
  await render(panel());
  expect(host.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toBe("Curator");
  expect(host.querySelector('[aria-label^="Heartbeat interval:"]')).toBeNull();
  await click("Heartbeat");
  const writes = () => requests.filter(({ init }) => init?.method === "POST");
  expect(JSON.parse(String(writes()[0].init?.body))).toEqual({ mode: "heartbeat" });
  expect(new Headers(writes()[0].init?.headers).get("X-CodeTerm-Plan-Confirmation")).toBe(FIXTURE_PLAN.id);
  expect(host.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toBe("Heartbeat");
  await click("Heartbeat interval");
  const options = document.body.querySelectorAll('[role="menuitemradio"]');
  expect(Array.from(options).map(node => node.textContent)).toEqual(["Machine default", "15m", "30m", "45m", "60m", "90m", "120m"]);
  await click("90m", document.body);
  expect(stored.heartbeat_interval_secs).toBe(5400);
  await click("Heartbeat interval");
  await click("Machine default", document.body);
  expect(stored.heartbeat_interval_secs).toBeNull();
  await click("Curator");
  expect(host.querySelector('[aria-label^="Heartbeat interval:"]')).toBeNull();
});

it("shows suspension and fallback states without changing the chosen mode", async () => {
  for (const suspended_reason of ["reminders_disabled", "waiting_on_human", "complete"]) {
    stored = { ...BASE, suspended_reason };
    await render(<ConnectedPlanSettings key={suspended_reason} plan={FIXTURE_PLAN} api={API} now={NOW} />);
    expect(host.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toBe("Curator");
    expect(host.textContent).not.toContain("Next round in");
    expect(host.textContent).toContain(suspended_reason === "reminders_disabled" ? "Settings > Agents" : suspended_reason === "complete" ? "Plan complete" : "Waiting for your answer");
  }
  stored = { ...BASE, fallback_reason: "provider_unavailable" };
  await render(<ConnectedPlanSettings key="fallback" plan={FIXTURE_PLAN} api={API} now={NOW} />);
  expect(host.textContent).toContain("heartbeat running");
  expect(host.textContent).toContain("provider unavailable");
  expect(requests.filter(({ init }) => init?.method === "POST")).toHaveLength(0);
});

it("moves Pause and Resume into the block and calls the lifecycle API", async () => {
  await render(panel());
  await click("Pause");
  expect(requests.some(({ path, init }) => path.endsWith("/pause") && init?.method === "POST")).toBe(true);
  await render(panel({ ...FIXTURE_PLAN, mode: "paused" }));
  expect(host.querySelector('[data-testid="plan-park-button"]')).toBeNull();
  await click("Resume");
  expect(requests.some(({ path, init }) => path.endsWith("/resume") && init?.method === "POST")).toBe(true);
});

it("opens the registered Curator Guide section", async () => {
  const open = vi.fn();
  await render(<GuideNavigationProvider sections={GUIDE_SECTIONS} onOpen={open}>{panel()}</GuideNavigationProvider>);
  await click("How the Curator works");
  expect(open).toHaveBeenCalledWith("curator");
  expect(getHelpContent().sections.find(section => section.id === "curator")).toBeDefined();
  expect(host.querySelector('[data-testid="mc-plan-settings-workers"]')).toBeNull();
  expect(host.querySelector('[data-testid="mc-plan-settings-quiet"]')).toBeNull();
});

it("shows refusal without switching the displayed persisted mode", async () => {
  await render(panel());
  vi.mocked(daemonFetch).mockResolvedValueOnce(new Response("fixture refusal", { status: 403 }));
  await click("Heartbeat");
  expect(host.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toBe("Curator");
  expect(host.querySelector('[role="alert"]')).toBeNull();
  expect(publishNotice).toHaveBeenCalledWith(expect.objectContaining({ kind: "toast", severity: "Error" }));
  expect(button("Heartbeat").disabled).toBe(false);
});

it("accepts live plan state updates and refreshes the countdown from its injected clock", async () => {
  await render(<ConnectedPlanSettings plan={FIXTURE_PLAN} initial={BASE} now={NOW} />);
  const before = host.textContent;
  await render(<ConnectedPlanSettings plan={FIXTURE_PLAN} initial={BASE} now={NOW + 30 * 60_000} />);
  expect(host.textContent).not.toBe(before);
  await render(<ConnectedPlanSettings plan={FIXTURE_PLAN} initial={{ ...BASE, mode: "heartbeat" }} now={NOW} />);
  expect(host.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toBe("Heartbeat");
});

it("disables mode changes without a connection and keeps round details out of task navigation", async () => {
  await render(<ConnectedPlanSettings plan={FIXTURE_PLAN} initial={BASE} now={NOW} />);
  expect(button("Heartbeat").disabled).toBe(true);
  const open = vi.fn();
  await render(<PlanRecentActivityRail plan={{ ...FIXTURE_PLAN, tasks: [], strides: [{ ...FIXTURE_MILESTONE, id: "round", ts: NOW, kind: "curator_round", text: "opaque round summary", from: { tab_id: null, session_id: null, title: "Curator" } }] }} onOpenTask={open} now={NOW} />);
  const row = host.querySelector('[data-testid="plan-recent-activity-item"]');
  expect(row?.textContent).toContain("Curator round");
  await act(async () => row?.querySelector<HTMLButtonElement>("button")?.click());
  expect(open).not.toHaveBeenCalled();
});

it("maps wire seconds, last round and the persisted active pause operation", () => {
  const result = planOversight({ ...FIXTURE_PLAN, mode: "paused", pause_operations: [{ paused_at: NOW - 1, reason: "fixture pause" }] }, { ...BASE, heartbeat_interval_secs: 5400, last_round: { number: 1, started_at: NOW - 10, ended_at: NOW, outcome: "done", summary: "", reason: "", next_delay_secs: 2700, learnings: 0, save_id: null } }, "loading");
  expect(result.heartbeat_interval_ms).toBe(5_400_000);
  expect(result.last_round_at).toBe(NOW);
  expect(result.paused_at).toBe(NOW - 1);
  expect(result.pause_reason).toBe("fixture pause");
});

it("renders Curator runs with the standard transcript action", async () => {
  const open = vi.fn();
  const run = { run_id: "fixture-run", started_at: NOW, finished_at: NOW + 1000, status: "Success" as const, tab_id: "fixture-tab", session_id: "fixture-session" };
  await render(<CronRunHistory runs={[run]} allowed onOpen={open} runLabel="Curator round" />);
  const transcript = host.querySelector<HTMLButtonElement>('button[aria-label^="View transcript:"]');
  expect(transcript?.getAttribute("aria-label")).toContain("Curator round");
  await act(async () => transcript?.click());
  expect(open).toHaveBeenCalledWith(run);
});


it("shows a load failure in one alert and keeps update failures off that surface", async () => {
  vi.mocked(daemonFetch).mockResolvedValueOnce(new Response("fixture load refusal", { status: 403 }));
  await render(panel());
  expect(host.querySelectorAll('[role="alert"]')).toHaveLength(1);
  expect(host.querySelector('[role="alert"] [role]')).toBeNull();
  expect(publishNotice).not.toHaveBeenCalled();
});

it("queues overlapping updates and uses write responses without refetching", async () => {
  let control: ReturnType<typeof usePlanControlLoop> | undefined;
  function Harness() {
    control = usePlanControlLoop(API, FIXTURE_PLAN.id, 1);
    return <span>{control.value?.heartbeat_interval_secs}</span>;
  }
  await render(<Harness />);
  let release: ((value: Response) => void) | undefined;
  vi.mocked(daemonFetch).mockImplementationOnce(() => new Promise(resolve => { release = resolve; }));
  let first: Promise<void> | undefined;
  let second: Promise<void> | undefined;
  await act(async () => {
    first = control!.update({ mode: "heartbeat" });
    second = control!.update({ mode: "heartbeat", heartbeat_interval_secs: 5400 });
  });
  expect(control!.busy).toBe(true);
  expect(requests.filter(({ init }) => init?.method !== "POST")).toHaveLength(1);
  await act(async () => {
    release!(Response.json({ ...BASE, mode: "heartbeat" }));
    await first;
    await second;
  });
  expect(control!.value?.heartbeat_interval_secs).toBe(5400);
  expect(control!.busy).toBe(false);
  expect(requests.filter(({ init }) => init?.method !== "POST")).toHaveLength(1);
});
