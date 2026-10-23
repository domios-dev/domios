import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it } from "vitest";
import { ChatBlockHostProvider, type ChatMentionValue } from "../blocks/ChatBlockHost";
import { CompactMessageContent } from "./CompactMessageContent";
import { ReferenceChip, TASK_CHIP_TITLE_CHARS, splitTaskLabel, truncateChipTitle } from "./ReferenceChip";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const LONG_TITLE = "Command/probe budgets: start at spawn return, split probe vs command-step defaults, per-step timeout (checker <=60s)";
const live: Record<string, ChatMentionValue> = {
  "BLU-44": { kind: "task", id: "BLU-44", title: `BLU-44 BLU-44 ${LONG_TITLE}`, planId: "pl_1", targetId: "task_1" },
};
const host = { mentionTabId: "tab", mentions: { binding: { subscribe: () => () => {}, read: (key: string) => live[(JSON.parse(key) as string[])[2]] ?? null, refresh: () => {} }, open: () => {} } };
const mounted: { root: Root; el: HTMLElement }[] = [];

async function mount(node: React.ReactNode): Promise<HTMLElement> {
  const el = document.createElement("div");
  document.body.append(el);
  const root = createRoot(el);
  mounted.push({ root, el });
  await act(async () => root.render(<ChatBlockHostProvider value={host}>{node}</ChatBlockHostProvider>));
  return el;
}

afterEach(async () => {
  for (const { root, el } of mounted.splice(0)) {
    await act(async () => root.unmount());
    el.remove();
  }
});

describe("task reference chip", () => {
  it("shows the key once and a bounded title, keeping the full title in the accessible name", async () => {
    const el = await mount(<ReferenceChip kind="task" label={`BLU-44 BLU-44 ${LONG_TITLE}`} fullRef="pl_1/task_1" disabled={false} />);
    const chip = el.querySelector("button")!;
    const visible = chip.textContent ?? "";
    expect(visible.match(/BLU-44/g)).toHaveLength(1);
    expect(visible.endsWith("…")).toBe(true);
    expect(visible.length).toBeLessThan(`BLU-44 ${LONG_TITLE}`.length);
    expect(chip.getAttribute("aria-label")).toContain(LONG_TITLE);
    expect(chip.getAttribute("title")).toContain(LONG_TITLE);
  });

  it("splits a label into key and title and truncates by the component's rule", () => {
    expect(splitTaskLabel("BLU-44 BLU-44 Short")).toEqual({ key: "BLU-44", title: "Short" });
    expect(splitTaskLabel("BLU-44")).toEqual({ key: "BLU-44", title: "" });
    expect(truncateChipTitle("short")).toBe("short");
    expect(truncateChipTitle("x".repeat(TASK_CHIP_TITLE_CHARS + 20))).toHaveLength(TASK_CHIP_TITLE_CHARS);
  });

  it("keeps a chip in a list item and its trailing colon in one inline run", async () => {
    const el = await mount(<CompactMessageContent markdown text={"What is left:\n\n- BLU-44: waiting for your choice\n"} onOpenPlan={() => {}} />);
    const item = el.querySelector("li")!;
    const chip = item.querySelector<HTMLElement>('[data-ref-kind="task"]')!;
    expect(chip).not.toBeNull();
    const run = chip.parentElement!;
    expect(run).not.toBe(item);
    expect(run.textContent?.endsWith(":")).toBe(true);
    expect(item.textContent).toContain(": waiting for your choice");
  });
});
