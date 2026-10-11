import { describe, expect, it } from "vitest";
import { sidebarDrawerLayout } from "./sidebarDrawerLayout";

describe("sidebar drawer layout", () => {
  it("anchors the phone drawer to the shell body below the titlebar", () => {
    expect(sidebarDrawerLayout(true)).toMatchObject({
      position: "absolute",
      top: 0,
      bottom: 0,
      left: 0,
    });
  });

  it("reserves the bottom safe area without applying the titlebar inset twice", () => {
    const layout = sidebarDrawerLayout(true);
    expect(layout.paddingBottom).toBe("var(--ct-safe-bottom, 0px)");
    expect(layout).not.toHaveProperty("paddingTop");
    expect(layout).not.toHaveProperty("height");
  });

  it("leaves desktop positioning and sizing to the existing flex layout", () => {
    expect(sidebarDrawerLayout(false)).toEqual({});
  });
});
