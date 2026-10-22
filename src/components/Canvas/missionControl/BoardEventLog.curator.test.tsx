import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import { BoardEventLog } from "./BoardEventLog";
import type { PlanEvent } from "./missionControlState";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("expands structured round details and retains every collapsed checklist task action", async () => {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const opened: string[] = [];
  const refs = new Map(["task_a", "task_b"].map(id => [id, { label: id, open: () => opened.push(id) }]));
  const base: PlanEvent = { ts: 100, last_ts: 100, count: 1, kind: "tab_todo_linked", author: "opaque", author_tab_id: null,
    task_id: "task_a", task_name: "task_a", text: "opaque checklist" };
  const detail = "opaque detail fragment";
  const log = { recorded_events: 3, truncated: false, entries: [base, { ...base, task_id: "task_b", task_name: "task_b" },
    { ...base, kind: "curator_round", task_id: null, task_name: null,
      text: JSON.stringify({ number: 2, outcome: "done", move_line: "opaque movement", retro: [detail], next_delay_secs: 3600 }) }] };
  try {
    await act(async () => root.render(<BoardEventLog log={log} milestoneKinds={[]} refs={refs} />));
    expect(host.querySelectorAll('[data-testid="mc-board-event-row"]')).toHaveLength(2);
    expect(host.textContent).not.toContain(detail);
    for (const id of refs.keys()) {
      const link = host.querySelector<HTMLButtonElement>(`button[data-ref-id="${id}"]`);
      expect(link).not.toBeNull();
      await act(async () => link!.click());
    }
    expect(opened).toEqual([...refs.keys()]);
    const toggle = host.querySelector<HTMLButtonElement>('button[aria-expanded="false"]');
    expect(toggle).not.toBeNull();
    await act(async () => toggle!.click());
    expect(host.textContent).toContain(detail);
    expect(host.textContent).not.toContain('"next_delay_secs"');
    await act(async () => toggle!.click());
    expect(host.textContent).not.toContain(detail);
  } finally { await act(async () => root.unmount()); host.remove(); }
});
