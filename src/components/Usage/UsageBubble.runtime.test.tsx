import assert from "node:assert/strict";
import {act} from "react";
import {createRoot} from "react-dom/client";
import {Window} from "happy-dom";
import type {UsageSnapshot,RuntimeUsageProjection} from "../../types/usage";
async function main() {
const dom=new Window({url:"http://localhost/"});
Object.defineProperty(globalThis,"IS_REACT_ACT_ENVIRONMENT",{value:true,configurable:true});
for(const key of ["window","document","navigator","HTMLElement","Element","Node","Event","MouseEvent"] as const) Object.defineProperty(globalThis,key,{value:dom[key],configurable:true});
const bridge=await import("../../lib/bridge");
let snapshot:UsageSnapshot;
bridge.__setInvokeForTest(async <T,>(command:string):Promise<T> => (command === "usage_latest" ? snapshot : command === "provider_model_usage" ? [] : null) as T);
bridge.__setListenForTest(async ()=>()=>{});
const {UsageBubble}=await import("./UsageBubble");
const container=document.createElement("div");document.body.append(container);const root=createRoot(container);
const reading:RuntimeUsageProjection={tab_id:"own",thread_id:"thread",billing_mode:"subscription",token_usage:null,rate_limits:{captured_at:Date.now(),value:[{window:"primary",usedPercent:2},{window:"secondary",usedPercent:0}]}};
async function render(mode:RuntimeUsageProjection){
 snapshot={provider:"fixture",captured_at:Date.now(),runtime_usage:[mode]} as UsageSnapshot;
 await act(async()=>root.render(<UsageBubble key={mode.billing_mode} provider="fixture" tabId="own" threadId="thread" trackingEnabled/>));
}
await render(reading);
assert.equal(container.querySelector("button")?.getAttribute("aria-label"),"usage session 2% weekly 0%");
await render({...reading,billing_mode:"api",token_usage:{captured_at:Date.now(),value:{input_tokens:100,output_tokens:20,cache_read_tokens:30,cost_micros:20000}}});
const button=container.querySelector("button")!;
assert.equal(button.getAttribute("aria-label"),"usage 120 tokens · $0.02");
await act(async()=>button.dispatchEvent(new MouseEvent("click",{bubbles:true})));
assert.match(container.querySelector('[role="status"]')?.textContent ?? "",/cached input: 30/);
await act(async()=>root.unmount());bridge.__setInvokeForTest(null);bridge.__setListenForTest(null);dom.happyDOM.abort();
console.log("PASS usage chip wiring: subscription metrics and API session breakdown");

}
void main().catch(error => { console.error(error); process.exitCode=1; });
