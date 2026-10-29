import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it } from "vitest";
import { PluginOperationApproval, usePluginOperationApproval } from "./PluginOperationApproval";

Object.assign(globalThis,{IS_REACT_ACT_ENVIRONMENT:true});

describe("host step approval bar", () => {
  it("focuses dismissal and records only explicit approval", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    let controller:ReturnType<typeof usePluginOperationApproval>|undefined;
    function Fixture() {
      controller = usePluginOperationApproval();
      return <PluginOperationApproval approval={controller.approval} pluginName="Fixture" onApprove={controller.approve} onDismiss={controller.dismiss}/>;
    }
    const quote = {amount:5,unit:"micro_credits",amountDisplay:"5 units (micro_credits)",runLabel:"Covers run #12",nodeLabel:"Generate",aboveRunBudget:true};
    try {
      await act(async () => root.render(<Fixture/>));
      let answer!:Promise<boolean>;
      await act(async () => {answer = controller!.confirm(quote);});
      expect(host.textContent).toContain("Above this run's budget.");
      expect(document.activeElement?.textContent).toBe("Not now");
      expect(controller!.isOpen()).toBe(true);
      await act(async () => window.dispatchEvent(new KeyboardEvent("keydown",{key:"Escape"})));
      expect(await answer!).toBe(false);
      expect(host.querySelector('[aria-label="Step approval"]')).toBeNull();
      await act(async () => {answer = controller!.confirm({...quote,aboveRunBudget:false});});
      expect(host.textContent).not.toContain("Above this run's budget.");
      await act(async () => controller!.dismiss());
      expect(await answer).toBe(false);
      await act(async () => {answer = controller!.confirm(quote);});
      await act(async () => [...host.querySelectorAll<HTMLButtonElement>("button")].find(button => button.textContent?.startsWith("Approve "))!.click());
      expect(await answer!).toBe(true);
      expect([...host.querySelectorAll<HTMLButtonElement>("button")].find(button => button.textContent?.startsWith("Approve "))!.disabled).toBe(true);
      await act(async () => controller!.settled({ok:false,code:"denied",reason:"stale"}));
      expect([...host.querySelectorAll("button")].some(button => button.textContent?.startsWith("Approve "))).toBe(false);
      await act(async () => controller!.dismiss());
      expect(controller!.isOpen()).toBe(false);
      await act(async () => {answer = controller!.confirm(quote);});
      await act(async () => controller!.dismiss());
      expect(await answer!).toBe(false);
    } finally {await act(async () => root.unmount());host.remove();}
  });
});

