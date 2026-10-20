import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it } from "vitest";
import { deferredComponent } from "./deferredComponent";

let panelRenders = 0;
function Panel({ label }: { label: string }) {
  panelRenders++;
  return <p>{label}</p>;
}

const DeferredPanel = deferredComponent(async () => ({ default: Panel }));

describe("deferred panels", () => {
  it("skip app shell renders that leave their props unchanged", async () => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    const host = document.createElement("div");
    const root = createRoot(host);
    function Shell({ tick, label }: { tick: number; label: string }) {
      return <section data-tick={tick}><DeferredPanel label={label} /></section>;
    }
    await act(async () => root.render(<Shell tick={0} label="Mesh" />));
    await act(async () => { await Promise.resolve(); });
    expect(host.textContent).toBe("Mesh");
    const before = panelRenders;
    await act(async () => root.render(<Shell tick={1} label="Mesh" />));
    expect(panelRenders).toBe(before);
    await act(async () => root.render(<Shell tick={2} label="Memory" />));
    expect(host.textContent).toBe("Memory");
    await act(async () => root.unmount());
  });
});
