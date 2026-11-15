import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { ConciergeRecoveryNotice } from "./NeighborPanel";
import { conciergeNeedsAttention, conciergeStatusLine, parseConciergeStatus, type ConciergeStatus } from "./useConciergeStatus";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let cleanup: (() => void) | undefined;
afterEach(() => cleanup?.());

it.each([
  ["concierge_auth_needed", "Open sign-in", "open"],
  ["concierge_access_needed", "Review access", "access"],
  ["concierge_provider_refused", "Review provider", "open"],
  ["concierge_invalid", "Ask General to fix", "setup"],
  ["concierge_crashed", "Open recovery", "open"],
  ["concierge_closed", "Set up with General", "setup"],
  ["concierge_missing", "Set up with General", "setup"],
] as const)("%s survives parsing and exposes exactly one recovery action", async (state, label, target) => {
  const status: ConciergeStatus = {
    state, gateway_tab: state === "concierge_closed" || state === "concierge_missing" ? null : "own-concierge",
    concierge_missing: state === "concierge_closed" || state === "concierge_missing", pending_count: 3,
    receiving_here: true, receiver_machine_label: "Here",
  };
  const parsed = parseConciergeStatus(status);
  expect(parsed).toEqual(status);
  expect(conciergeNeedsAttention(parsed)).toBe(true);
  expect(conciergeStatusLine(parsed)).not.toMatch(/connected|starting/);
  const actions = { setup: vi.fn(), open: vi.fn(), access: vi.fn() };
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  cleanup = () => { act(() => root.unmount()); host.remove(); };
  await act(async () => root.render(<ConciergeRecoveryNotice status={status} friendName="Fixture friend" onSetUpWithGeneral={actions.setup}
    onOpenConcierge={actions.open} onReviewAccess={actions.access} />));
  const notice = host.querySelector('[role="status"]')!;
  const buttons = notice.querySelectorAll("button");
  expect(buttons).toHaveLength(1);
  expect(buttons[0].textContent).toBe(label);
  if (state === "concierge_invalid") expect(notice.textContent).toContain("Fixture friend");
  await act(async () => buttons[0].click());
  expect(actions[target]).toHaveBeenCalledTimes(1);
  if (target === "open") expect(actions.open).toHaveBeenCalledWith("own-concierge");
  for (const [name, action] of Object.entries(actions)) if (name !== target) expect(action).not.toHaveBeenCalled();
});

it.each(["ready", "concierge_starting", "paused", "receiving_elsewhere", "not_receiving"] as const)("%s has no recovery notice", async state => {
  const host = document.createElement("div");
  const root = createRoot(host);
  cleanup = () => act(() => root.unmount());
  const action = vi.fn();
  await act(async () => root.render(<ConciergeRecoveryNotice friendName="Fixture friend" status={{ state, gateway_tab: "tab", concierge_missing: false,
    pending_count: null, receiving_here: true, receiver_machine_label: null }} onSetUpWithGeneral={action}
    onOpenConcierge={action} onReviewAccess={action} />));
  expect(host.querySelector('[role="status"]')).toBeNull();
  expect(action).not.toHaveBeenCalled();
});
