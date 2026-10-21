import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { ErrorNotice, InlineErrorNotice } from "./ErrorNotice";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("wires the supplied title, actions, collapsed raw disclosure and copy control", async () => {
  const run = vi.fn();
  const parentClick = vi.fn();
  const parentKey = vi.fn();
  const copy = vi.fn(async () => {});
  vi.stubGlobal("navigator", { clipboard: { writeText: copy } });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const description = { title: "fixture-title", cause: "fixture-cause", raw: "<raw>\nopaque-fault", actions: [] };
  try {
    await act(async () => root.render(<div onClick={parentClick} onKeyDown={parentKey}><ErrorNotice description={description} actions={[{ label: "fixture-action", onClick: run }]} blocking /></div>));
    expect(host.querySelector('[role="alert"]')?.textContent).toContain(description.title);
    const button = host.querySelector<HTMLButtonElement>('button[aria-label="fixture-action"]')!;
    await act(async () => button.click());
    expect(run).toHaveBeenCalledTimes(1);
    const details = host.querySelector("details")!;
    expect(details.open).toBe(false);
    await act(async () => details.querySelector("summary")!.click());
    expect(details.open).toBe(true);
    expect(details.querySelector("pre")?.textContent).toBe(description.raw);
    await act(async () => details.querySelector("button")!.click());
    expect(copy).toHaveBeenCalledExactlyOnceWith(description.raw);
    expect(parentClick).not.toHaveBeenCalled();
    details.querySelector("summary")!.dispatchEvent(new KeyboardEvent("keydown", { code: "Enter", bubbles: true }));
    expect(parentKey).not.toHaveBeenCalled();
    details.querySelector("summary")!.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true }));
    expect(parentKey).toHaveBeenCalledTimes(1);
    await act(async () => root.render(<InlineErrorNotice description={description} blocking actions={[{ label: "fixture-action", onClick: run }]} />));
    expect(host.querySelector('[role="alert"]')).not.toBeNull();
    expect(host.querySelector("details")?.open).toBe(false);
    expect(host.querySelector("pre")?.textContent).toBe(description.raw);
    await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="fixture-action"]')!.click());
    expect(run).toHaveBeenCalledTimes(2);
  } finally {
    await act(async () => root.unmount());
    host.remove();
    vi.unstubAllGlobals();
  }
});

it("keeps focus in the adjacent form when a blocking inline failure appears", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const form = (failed: boolean) => <><input aria-label="fixture input" />{failed && <InlineErrorNotice error="opaque refusal" operation="saving the form" blocking actions={[{ label: "Retry", onClick: vi.fn() }]} />}</>;
  try {
    await act(async () => root.render(form(false)));
    const input = host.querySelector("input")!; input.focus();
    await act(async () => root.render(form(true)));
    expect(document.activeElement).toBe(input);
    expect(host.querySelector('[role="alert"]')).not.toBeNull();
    expect(host.querySelector("details")?.open).toBe(false);
  } finally { await act(async () => root.unmount()); host.remove(); }
});
