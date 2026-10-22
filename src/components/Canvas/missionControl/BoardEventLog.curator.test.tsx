import { EVENT_LINKED_TASKS_PREVIEW, EVENT_PART_SEPARATOR } from "./missionControlBoardModel";
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
  const log = { recorded_events: 3, truncated: false, entries: [{ ...base, count: 2, linked_tasks: [...refs.keys()].map(id => ({ id, name: id })) },
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

it("bounds checklist controls until expanded, separates labels, and exposes every task action", async () => {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const opened: string[] = [];
  const tasks = Array.from({ length: 20 }, (_, index) => ({ id: `task_${index}`, name: `opaque ${index}` }));
  const refs = new Map(tasks.map(task => [task.id, { label: task.name, open: () => opened.push(task.id) }]));
  const entry: PlanEvent = { ts: 1, last_ts: 20, count: 20, kind: "tab_todo_linked", author: "", author_tab_id: null,
    task_id: null, task_name: null, linked_tasks: tasks, text: "opaque" };
  try {
    await act(async () => root.render(<BoardEventLog log={{ recorded_events: 20, truncated: false, entries: [entry] }} milestoneKinds={[]} refs={refs} />));
    expect(host.querySelectorAll("button[data-ref-id]")).toHaveLength(EVENT_LINKED_TASKS_PREVIEW);
    expect(host.textContent).toContain(`+${tasks.length - EVENT_LINKED_TASKS_PREVIEW}`);
    expect(host.textContent).toContain(tasks[0].name + EVENT_PART_SEPARATOR + tasks[1].name);
    const toggle = host.querySelector<HTMLButtonElement>('button[aria-expanded="false"]')!;
    expect(toggle).not.toBeNull();
    await act(async () => toggle.click());
    expect(host.querySelectorAll("button[data-ref-id]")).toHaveLength(tasks.length);
    for (const button of host.querySelectorAll<HTMLButtonElement>("button[data-ref-id]")) await act(async () => button.click());
    expect(opened).toEqual(tasks.map(task => task.id));
    await act(async () => toggle.click());
    expect(host.querySelectorAll("button[data-ref-id]")).toHaveLength(EVENT_LINKED_TASKS_PREVIEW);
  } finally { await act(async () => root.unmount()); host.remove(); }
});
