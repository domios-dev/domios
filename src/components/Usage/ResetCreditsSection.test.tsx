import assert from "node:assert/strict";
import {act} from "react";
import {createRoot} from "react-dom/client";
import {Window} from "happy-dom";
import {ResetCreditsSection} from "./ResetCreditsSection";
import {ResetCreditsModel} from "./resetCreditsModel";

async function main() {
  const window = new Window();
  Object.assign(globalThis, {window,document:window.document,HTMLElement:window.HTMLElement,IS_REACT_ACT_ENVIRONMENT:true});
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  let calls = 0;
  const selectedIds: (string|undefined)[] = [];
  const model = new ResetCreditsModel(() => "fixture", async (_key,id) => {calls++;selectedIds.push(id);return {outcome:"reset"};});
  const button = (name:string) => {
    const match = [...document.querySelectorAll("button")].find(item => item.textContent === name);
    assert.ok(match, `button ${name}`); return match;
  };
  await act(async () => root.render(<ResetCreditsSection model={model} />));
  assert.equal(host.querySelector("section"),null);
  await act(async () => root.render(<ResetCreditsSection model={model} resets={{availableCount:0,nextExpiry:null,credits:null}} />));
  assert.equal(button("Reset now").disabled,true);
  await act(async () => root.render(<ResetCreditsSection model={model} resets={{availableCount:1,nextExpiry:2_000_000_000_000,credits:null}} />));
  await act(async () => button("Reset now").click());
  assert.ok(document.querySelector('[role="dialog"]'));
  assert.equal(calls,0);
  await act(async () => button("Cancel").click());
  assert.equal(calls,0);
  await act(async () => button("Reset now").click());
  await act(async () => button("Confirm reset").click());
  assert.equal(calls,1);
  assert.ok(host.querySelector('[role="status"]'));
  assert.equal(model.outcome,"reset");
  const rows = {availableCount:3,nextExpiry:null,credits:[
    {id:"gift-a",title:"Fixture grant",description:"Fixture description",status:"available" as const,expiresAt:2_000_000_000_000},
    {id:"gift-b",status:"available" as const,expiresAt:null},
    {id:"used",status:"redeemed" as const,expiresAt:null},
  ]};
  await act(async () => root.render(<ResetCreditsSection model={model} resets={rows} />));
  const first = host.querySelector<HTMLInputElement>('input[type="radio"][aria-label="Fixture grant · gift-a"]');
  const second = host.querySelector<HTMLInputElement>('input[type="radio"][aria-label="gift-b"]');
  assert.ok(first); assert.ok(second);
  assert.equal(host.querySelectorAll('input[type="radio"]').length,3);
  await act(async () => second.click());
  assert.equal(second.checked,true);
  assert.equal(first.checked,false);
  await act(async () => button("Reset now").click());
  assert.equal(calls,1);
  await act(async () => button("Confirm reset").click());
  assert.deepEqual(selectedIds,[undefined,"gift-b"]);
  await act(async () => root.unmount());
  console.log("PASS reset section accessible controls require confirmation and show outcome");
}
void main();
