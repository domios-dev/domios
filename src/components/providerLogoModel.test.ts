import { describe, expect, it } from "vitest";
import { initialProviderLogoState, providerLogoPresentation } from "./providerLogoModel";

describe("provider logo presentation", () => {
  it.each(["loading", "failed", "absent"] as const)("uses the placeholder when %s", (state) => {
    expect(providerLogoPresentation(state)).toBe("placeholder");
  });
  it("uses the logo when available", () => {
    expect(providerLogoPresentation("available")).toBe("logo");
  });
  it.each([undefined, null, "", "  "])("classifies missing SVG as absent", (svg) => {
    expect(initialProviderLogoState(svg)).toBe("absent");
  });
  it("classifies inline vector logos as immediately available", () => {
    expect(initialProviderLogoState('<svg><path d="M0 0"/></svg>')).toBe("available");
  });
  it("waits for embedded image load events", () => {
    expect(initialProviderLogoState('<svg><image href="logo.png"/></svg>')).toBe("loading");
  });
  it("classifies malformed logo content as failed", () => {
    expect(initialProviderLogoState("broken logo")).toBe("failed");
  });
});
