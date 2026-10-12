import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it } from "vitest";
import { ProviderGlyph } from "./ProviderGlyph";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const host = document.createElement("div");
document.body.append(host);
const root = createRoot(host);
afterEach(async () => { await act(async () => root.render(null)); });

function imageNamed(name: string) {
  return host.querySelector(`[role="img"][aria-label="${name}"]`);
}

describe("ProviderGlyph", () => {
  it("replaces a missing descriptor logo when it arrives", async () => {
    await act(async () => root.render(<ProviderGlyph iconHint="OC" />));
    expect(imageNamed("Default agent icon")).not.toBeNull();
    await act(async () => root.render(<ProviderGlyph iconSvg="<svg><path /></svg>" title="Test provider" />));
    expect(imageNamed("Test provider")).not.toBeNull();
    expect(imageNamed("Test provider default agent icon")).toBeNull();
  });

  it("keeps the placeholder until an embedded logo loads", async () => {
    await act(async () => root.render(<ProviderGlyph iconSvg='<svg><image href="logo.png"/></svg>' title="Test provider" />));
    expect(imageNamed("Test provider default agent icon")).not.toBeNull();
    await act(async () => host.querySelector("image")!.dispatchEvent(new Event("load")));
    expect(imageNamed("Test provider")).not.toBeNull();
  });

  it("exposes the placeholder after load failure and recovers for a new logo", async () => {
    await act(async () => root.render(<ProviderGlyph iconSvg='<svg><image href="failed.png"/></svg>' title="Test provider" />));
    await act(async () => host.querySelector("image")!.dispatchEvent(new Event("error")));
    expect(imageNamed("Test provider default agent icon")).not.toBeNull();
    await act(async () => host.querySelector("image")!.dispatchEvent(new Event("load")));
    expect(imageNamed("Test provider default agent icon")).not.toBeNull();
    await act(async () => root.render(<ProviderGlyph iconSvg="<svg><circle /></svg>" title="Test provider" />));
    expect(imageNamed("Test provider")).not.toBeNull();
  });
});
