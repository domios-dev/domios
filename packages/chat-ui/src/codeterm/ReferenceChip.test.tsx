import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it } from "vitest";
import { ChatBlockHostProvider, type ChatMentionValue } from "../blocks/ChatBlockHost";
import { CompactMessageContent } from "./CompactMessageContent";
import { referenceMarkerText } from "./referenceMarkers";
import { ReferenceChip, TASK_CHIP_TITLE_CHARS, splitTaskLabel, truncateChipTitle } from "./ReferenceChip";
import { MessageBubble } from "../MessageBubble";

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
    expect(chip.getAttribute("title")).toBeNull();
    await act(async () => chip.focus());
    expect(document.querySelector('[role="tooltip"]')?.textContent).toContain(LONG_TITLE);
  });

  it("splits a label into key and title and truncates by the component's rule", () => {
    expect(splitTaskLabel("BLU-44 BLU-44 Short")).toEqual({ key: "BLU-44", title: "Short" });
    expect(splitTaskLabel("BLU-44")).toEqual({ key: "BLU-44", title: "" });
    expect(truncateChipTitle("short")).toBe("short");
    expect(truncateChipTitle("x".repeat(TASK_CHIP_TITLE_CHARS + 20))).toHaveLength(TASK_CHIP_TITLE_CHARS);
  });

  it("keeps a chip in a list item and its trailing colon in one inline run", async () => {
    // Task chips come only from explicit markers; a bare key stays text.
    const el = await mount(<CompactMessageContent markdown text={`What is left:\n\n- ${referenceMarkerText("task", "BLU-44")}: waiting for your choice\n`} onOpenPlan={() => {}} />);
    const item = el.querySelector("li")!;
    const chip = item.querySelector<HTMLElement>('[data-ref-kind="task"]')!;
    expect(chip).not.toBeNull();
    expect(item.textContent).toContain(`${chip.textContent}: waiting for your choice`);
  });
});

describe("participant mentions", () => {
  it("preserves resolved, closed, loading and missing semantics in the production user bubble", async () => {
    live["review-live"] = { kind: "tab", id: "review-live", title: "Elgar", tabKind: "agent", providerName: "Claude", activity: "working" };
    live["review-closed"] = { kind: "tab", id: "review-closed", title: "Closed", state: "closed" };
    live["review-loading"] = { kind: "tab", id: "review-loading", title: "opaque", state: "loading" };
    const content = ["review-live", "review-closed", "review-loading", "review-missing"].map(id => referenceMarkerText("tab", id)).join(" ");
    const el = await mount(<MessageBubble message={{ id: "review", type: "user", content }} />);
    const bubble = el.querySelector('[role="group"][aria-label="User message"]')!;
    expect([...bubble.querySelectorAll("button")].map(button => [button.getAttribute("aria-label"), button.getAttribute("aria-disabled")])).toEqual([
      ["Elgar, Claude agent tab, working", null], ["Closed, closed tab", "true"], ["Tab, loading tab", "true"],
    ]);
    expect(bubble.textContent).toBe("Elgar Closed Tab Unknown tab");
  });
  it("opens an agent through the host and exposes its live identity and status", async () => {
    let opened = 0;
    const el = await mount(<ReferenceChip kind="tab" presentation="mention" label="Elgar" fullRef="agent" disabled={false}
      participant={{ tabKind: "agent", providerName: "Claude", role: "Orchestrator", workspaceName: "codeterm", activity: "working" }} onOpen={() => opened++} />);
    const chip = el.querySelector("button")!;
    expect(chip.textContent).toBe("Elgar");
    expect(chip.getAttribute("aria-label")).toBe("Elgar, Claude agent tab, working");
    await act(async () => { chip.click(); chip.focus(); });
    expect(opened).toBe(1);
    expect(document.querySelector('[role="tooltip"]')?.textContent).toContain("Elgar · Orchestrator · Claude · codeterm");
    expect(chip.tabIndex).toBe(0);
  });

  it("keeps closed and loading chips focusable with status in the name and blocks activation", async () => {
    let opened = 0;
    const el = await mount(<>
      <ReferenceChip kind="tab" presentation="mention" label="build" fullRef="closed" state="closed" disabled onOpen={() => opened++} />
      <ReferenceChip kind="tab" presentation="mention" label="opaque-id" fullRef="pending" state="loading" disabled />
    </>);
    const [closed, pending] = Array.from(el.querySelectorAll("button"));
    expect(closed.disabled).toBe(false);
    expect(closed.getAttribute("aria-disabled")).toBe("true");
    expect(closed.getAttribute("aria-label")).toBe("build, closed tab");
    await act(async () => { closed.click(); closed.focus(); });
    expect(opened).toBe(0);
    expect(document.querySelector('[role="tooltip"]')?.textContent).toBe("build · Closed");
    expect(pending.textContent).toBe("Tab");
    expect(pending.getAttribute("aria-label")).toBe("Tab, loading tab");
  });

  it("renders unknown ids as plain text, preserves full long names and names shell/Browser/waiting/error states", async () => {
    const el = await mount(<>
      <ReferenceChip kind="tab" presentation="mention" label="id" fullRef="unknown" state="missing" disabled />
      <ReferenceChip kind="tab" presentation="mention" label="build" fullRef="shell" disabled={false} participant={{ tabKind: "shell" }} />
      <ReferenceChip kind="tab" presentation="mention" label="Browser" fullRef="browser" disabled={false} participant={{ tabKind: "browser" }} />
      <ReferenceChip kind="tab" presentation="mention" label={LONG_TITLE} fullRef="long" disabled={false} participant={{ tabKind: "agent", activity: "waiting" }} />
      <ReferenceChip kind="tab" presentation="mention" label="Broken" fullRef="error" disabled={false} participant={{ tabKind: "agent", activity: "error" }} />
    </>);
    expect(el.querySelector('[data-ref-state="missing"]')?.tagName).toBe("SPAN");
    expect(el.textContent).toContain("Unknown tab");
    const buttons = Array.from(el.querySelectorAll("button"));
    expect(buttons.map(button => button.getAttribute("aria-label"))).toEqual([
      "build, shell tab", "Browser, Browser tab", `${LONG_TITLE}, agent tab, waiting for you`, "Broken, agent tab, error",
    ]);
    expect(buttons[2].textContent?.length).toBeLessThanOrEqual(24);
    expect(buttons[2].textContent?.endsWith("…")).toBe(true);
    await act(async () => buttons[2].focus());
    expect(document.querySelector('[role="tooltip"]')?.textContent).toContain(LONG_TITLE);
  });
});
