import { describe, expect, it, vi } from "vitest";
import { createViewHostGate } from "./pluginViewHostActions";
import { openPluginRun } from "./pluginRunNavigation";

const reference = "a".repeat(64);
const intent = {shown:true,frameFocused:true,activated:true};
const destination = {workspaceId:"workspace",target:{blueprintId:"blueprint",runId:"run",teamId:null}};

describe("plugin run navigation", () => {
  it("opens the exact host target once through the shared per-frame gate", async () => {
    const gate = createViewHostGate(() => 1000);
    const open = vi.fn(); const resolve = vi.fn(async () => destination);
    expect(await openPluginRun(reference, intent, () => true, gate, {open,resolve})).toEqual({ok:true});
    expect(open).toHaveBeenCalledWith(destination);
    expect(await openPluginRun(reference, intent, () => true, gate, {open,resolve})).toEqual({ok:false,code:"denied",reason:"busy"});
    expect(open).toHaveBeenCalledTimes(1);
    expect(resolve).toHaveBeenCalledTimes(1);
  });
  it.each(["shown","frameFocused","activated"] as const)("refuses missing %s before resolving anything", async (missing) => {
    const resolve = vi.fn(); const open = vi.fn();
    expect(await openPluginRun(reference, {...intent,[missing]:false}, () => true, createViewHostGate(), {resolve,open})).toEqual({ok:false,code:"denied",reason:"gesture"});
    expect(resolve).not.toHaveBeenCalled(); expect(open).not.toHaveBeenCalled();
  });
  it("refuses a late response from a retired document without switching views", async () => {
    let current = true;
    let complete!: (value: unknown) => void;
    const pending = new Promise<unknown>((resolve) => {complete=resolve;});
    const open = vi.fn();
    const result = openPluginRun(reference, intent, () => current, createViewHostGate(), {resolve:() => pending,open});
    current = false; complete(destination);
    expect(await result).toEqual({ok:false,code:"denied",reason:"stale"});
    expect(open).not.toHaveBeenCalled();
  });
  it("rejects malformed targets and caller-made references", async () => {
    const open = vi.fn(); const resolve = vi.fn(async () => ({...destination,target:{...destination.target,runId:"../foreign"}}));
    expect(await openPluginRun(reference, intent, () => true, createViewHostGate(), {resolve,open})).toEqual({ok:false,code:"unavailable",reason:"run"});
    expect(await openPluginRun("../foreign", intent, () => true, createViewHostGate(), {resolve,open})).toEqual({ok:false,code:"invalid",reason:"reference"});
    expect(open).not.toHaveBeenCalled();
  });
});
