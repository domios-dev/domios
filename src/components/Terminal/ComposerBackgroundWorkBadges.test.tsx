import { useState } from "react";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { AgentInspectionProvider } from "@codeterm/chat-ui";
import { ComposerBackgroundWorkBadges } from "./ComposerBackgroundWorkBadges";
import type { ComposerBackgroundWorkItem } from "./composerBackgroundWorkModel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const agent: ComposerBackgroundWorkItem = {
  id:"agent:child",kind:"agent",name:"worker",threadId:"child",startedAtMs:1,
  prompt:"Review the route",model:"fixture-model",effort:"high",status:"running",
};
const terminal: ComposerBackgroundWorkItem = {
  id:"terminal:root:process",kind:"terminal",name:"node",threadId:"root",startedAtMs:1,
  processId:"process",command:"node build.mjs",cwd:"C:/work",osPid:42,outputTail:"finished line",
};

function accessibleName(button: HTMLButtonElement): string {
  return button.getAttribute("aria-label")?.trim() || button.textContent?.trim() || "";
}

it("opens an accessible subagent panel and requests its read-only transcript", async () => {
  const host = document.createElement("div");
  const root = createRoot(host);
  const inspect = vi.fn();
  try {
    await act(async () => root.render(
      <AgentInspectionProvider onInspect={inspect}>
        <ComposerBackgroundWorkBadges items={[agent]} error={null} onStop={vi.fn(async () => {})} />
      </AgentInspectionProvider>,
    ));
    const badge = host.querySelector<HTMLButtonElement>("button");
    expect(badge).toBeDefined();
    expect(accessibleName(badge!)).not.toBe("");
    expect(badge?.getAttribute("aria-haspopup")).toBe("dialog");
    await act(async () => badge?.click());
    const dialog = host.querySelector('[role="dialog"]');
    expect(dialog).not.toBeNull();
    const transcriptAction = dialog?.querySelector<HTMLButtonElement>("button");
    expect(transcriptAction).toBeDefined();
    expect(accessibleName(transcriptAction!)).not.toBe("");
    await act(async () => transcriptAction?.click());
    expect(inspect).toHaveBeenCalledExactlyOnceWith("child");
  } finally {
    await act(async () => root.unmount());
  }
});

it("stops a terminal once and removes its badge after the returned list omits it", async () => {
  const host = document.createElement("div");
  const root = createRoot(host);
  const stop = vi.fn(async (_item: ComposerBackgroundWorkItem) => {});
  function Harness() {
    const [items, setItems] = useState([terminal]);
    return <ComposerBackgroundWorkBadges items={items} error={null} onStop={async (item) => {
      await stop(item);
      setItems((current) => current.filter((entry) => entry.id !== item.id));
    }} />;
  }
  try {
    await act(async () => root.render(<Harness />));
    const badge = host.querySelector<HTMLButtonElement>("button");
    expect(badge).toBeDefined();
    expect(accessibleName(badge!)).not.toBe("");
    await act(async () => badge?.click());
    const dialog = host.querySelector('[role="dialog"]');
    expect(dialog).not.toBeNull();
    const stopButton = dialog?.querySelector<HTMLButtonElement>("button");
    expect(stopButton).toBeDefined();
    expect(accessibleName(stopButton!)).not.toBe("");
    await act(async () => {
      stopButton?.click();
      stopButton?.click();
    });
    expect(stop).toHaveBeenCalledExactlyOnceWith(terminal);
    expect(host.querySelector('[role="dialog"]')).toBeNull();
  } finally {
    await act(async () => root.unmount());
  }
});
