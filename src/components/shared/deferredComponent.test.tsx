import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import { deferredComponent } from "./deferredComponent";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("waits for the module and wires the loaded control once", async () => {
  let resolve!: (module: { default: typeof Control }) => void;
  let loads = 0;
  let clicks = 0;
  function Control({ onClick }: { onClick: () => void }) {
    return <button aria-label="Fixture action" onClick={onClick} />;
  }
  const module = new Promise<{ default: typeof Control }>(done => { resolve = done; });
  const View = deferredComponent(() => { loads++; return module; });
  expect(loads).toBe(0);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<View onClick={() => clicks++} />));
    expect(host.querySelector('[role="status"]')).not.toBeNull();
    expect(host.querySelector("button")).toBeNull();
    await act(async () => { resolve({ default: Control }); await module; });
    expect(host.querySelector('[role="status"]')).toBeNull();
    const button = host.querySelector<HTMLButtonElement>('button[aria-label="Fixture action"]');
    expect(button).not.toBeNull();
    await act(async () => button!.click());
    await act(async () => root.render(<View onClick={() => clicks++} />));
    expect(clicks).toBe(1);
    expect(loads).toBe(1);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
