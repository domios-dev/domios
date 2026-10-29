import { describe, expect, it, vi } from "vitest";
import { createViewHostGate } from "./pluginViewHostActions";
import { decidePluginOperation } from "./pluginOperationDecision";

const operation = "a".repeat(64);
const decision = {operation,decisionKey:`blueprint-plugin:${operation}:${"b".repeat(64)}`,action:"approve"};
const intent = {shown:true,frameFocused:true,activated:true};
const preview = {ok:true,preview:{operation,decisionKey:decision.decisionKey,runLabel:"Fixture run #1",nodeLabel:"Generate",amount:5,unit:"micro_credits",amountDisplay:"5 units (micro_credits)"}};
function host() {return {send:vi.fn(async (request: {action:string}) => request.action === "preview" ? preview : {ok:true}),confirm:vi.fn(() => true)};}

describe("plugin operation owner decisions", () => {
  it("requires an observed gesture and host confirmation of the exact daemon quote", async () => {
    const h = host();
    expect(await decidePluginOperation(decision,{...intent,activated:false},() => true,createViewHostGate(),h)).toEqual({ok:false,code:"denied",reason:"gesture"});
    expect(h.send).not.toHaveBeenCalled();
    expect(await decidePluginOperation(decision,intent,() => true,createViewHostGate(),h)).toEqual({ok:true});
    expect(h.confirm).toHaveBeenCalledWith({amount:5,unit:"micro_credits",amountDisplay:"5 units (micro_credits)",runLabel:"Fixture run #1",nodeLabel:"Generate",aboveRunBudget:false});
    expect(h.send.mock.calls).toEqual([[{...decision,action:"preview"}],[decision]]);
  });
  it("refuses widened authority and mismatched quote keys before the daemon call", async () => {
    const h = host();
    for (const value of [{...decision,maximum:1000},{...decision,owner:"foreign"},{...decision,decisionKey:`blueprint-plugin:${"c".repeat(64)}:${"b".repeat(64)}`}]) {
      expect(await decidePluginOperation(value,intent,() => true,createViewHostGate(),h)).toEqual({ok:false,code:"invalid",reason:"decision"});
    }
    expect(h.send).not.toHaveBeenCalled();
  });
  it("never approves a malformed quote or a declined confirmation", async () => {
    const h = host();
    h.confirm.mockReturnValue(false);
    expect(await decidePluginOperation(decision,intent,() => true,createViewHostGate(),h)).toEqual({ok:false,code:"denied",reason:"cancelled"});
    expect(h.send).toHaveBeenCalledTimes(1);
    const malformed = {...host(),send:vi.fn(async () => ({...preview,preview:{...preview.preview,amount:Number.MAX_SAFE_INTEGER+1}}))};
    expect(await decidePluginOperation(decision,intent,() => true,createViewHostGate(),malformed)).toEqual({ok:false,code:"denied",reason:"quote"});
    expect(malformed.confirm).not.toHaveBeenCalled();
  });
  it("rechecks document identity after preview and shares the per-frame gate", async () => {
    const gate = createViewHostGate(() => 1000);
    let current = true;
    const h = {...host(),send:vi.fn(async () => {current=false;return preview;})};
    expect(await decidePluginOperation(decision,intent,() => current,gate,h)).toEqual({ok:false,code:"denied",reason:"stale"});
    expect(h.confirm).not.toHaveBeenCalled();
    expect(await decidePluginOperation(decision,intent,() => true,gate,h)).toEqual({ok:false,code:"denied",reason:"busy"});
    expect(h.send).toHaveBeenCalledTimes(1);
  });
  it("free checks do not request spending confirmation", async () => {
    const h = host();
    expect(await decidePluginOperation({...decision,action:"check_price"},intent,() => true,createViewHostGate(),h)).toEqual({ok:true});
    expect(h.confirm).not.toHaveBeenCalled();
    expect(h.send).toHaveBeenCalledTimes(1);
  });
});

