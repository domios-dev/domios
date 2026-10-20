import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { ProviderUpdateNotice } from "./ProviderUpdateBubble";

vi.mock("../../hooks/useProviderCliStatus",()=>({useProviderCliStatus:()=>({status:{required:true,installed:true,latest_version:"1.2.3",updater:{},display_name:"fixture"},refresh:vi.fn()})}));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const host = document.createElement("div");
document.body.appendChild(host);
const root = createRoot(host);
afterEach(async () => { await act(async () => root.render(null)); });

it("offers update and dismissal and displays the model result in the same status", async () => {
  const update = vi.fn(), dismiss = vi.fn();
  const model = { key: "1.2.3", state: "available" as const, message: "opaque offer", canUpdate: true };
  await act(async () => root.render(<ProviderUpdateNotice model={model} onUpdate={update} onDismiss={dismiss} />));
  const buttons = [...host.querySelectorAll("button")];
  const action = buttons.find(button => button.textContent === "Update");
  expect(action).toBeDefined();
  await act(async () => action!.click());
  expect(update).toHaveBeenCalledTimes(1);
  await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="Dismiss provider update"]')!.click());
  expect(dismiss).toHaveBeenCalledTimes(1);
  await act(async () => root.render(<ProviderUpdateNotice model={{ ...model, state: "succeeded", message: "opaque result", canUpdate: false }} onUpdate={update} onDismiss={dismiss} />));
  expect(host.querySelector('[role="status"]')?.textContent).toBe("opaque result");
  expect([...host.querySelectorAll("button")].some(button => button.textContent === "Update")).toBe(false);
});

it("places provider updates before and outside the complete composer group", async () => {
  const { ChatComposerSurface } = await import("./ChatComposerSurface");
  await act(async () => root.render(<ChatComposerSurface view={{kind:"none"}} providerId="fixture" tabId="parent"><button type="button">Plan fixture</button><textarea aria-label="Draft" /></ChatComposerSurface>));
  const notice=host.querySelector('[role="group"][aria-label="Provider CLI update"]')!;
  const composer=host.querySelector('[role="group"][aria-label="Message composer"]')!;
  expect(notice).not.toBeNull();
  expect(composer.contains(notice)).toBe(false);
  expect(Boolean(notice.compareDocumentPosition(composer) & Node.DOCUMENT_POSITION_FOLLOWING)).toBe(true);
  expect(composer.querySelector('textarea[aria-label="Draft"]')).not.toBeNull();
});
