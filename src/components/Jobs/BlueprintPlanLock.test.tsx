import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it } from "vitest";
import { JobRow } from "./JobRow";
import { BlueprintPlanLock } from "../Automation/BlueprintPlanLock";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

describe("Blueprint plan hold badge", () => {
  it("exposes the lock in Jobs and clears it on unlock", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    let opened = 0;
    const row = (heldReason: string | null) => <JobRow jobKey="watch" icon={null} name="Watcher" meta="Changes" type="Blueprint" tone="blueprint" state={heldReason ? "Held" : "Active"} heldReason={heldReason} actionItems={[]} onOpen={() => { opened++; }} onTriggerRef={() => {}} error={null} />;
    try {
      await act(async () => root.render(row("plan_limit_locked")));
      expect(host.querySelector('[role="status"][aria-label="Blueprint locked by plan"]')).not.toBeNull();
      await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
      expect(opened).toBe(1);
      await act(async () => root.render(row(null)));
      expect(host.querySelector('[aria-label="Blueprint locked by plan"]')).toBeNull();
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });

  it("distinguishes a plan lock from an uncertain effect hold", async () => {
    const host = document.createElement("div");
    const root = createRoot(host);
    try {
      await act(async () => root.render(<BlueprintPlanLock reason="uncertain_pipeline_effect" />));
      expect(host.textContent).toBe("");
      await act(async () => root.render(<BlueprintPlanLock reason="plan_limit_locked" />));
      expect(host.querySelector('[aria-label="Blueprint locked by plan"]')).not.toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  });
});
