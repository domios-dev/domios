import { AppWindow, Globe, LayoutDashboard } from "lucide-react";
import { describe, expect, it } from "vitest";
import { surfaceIconComponent } from "./surfaceIcon";
import { parseDeclaredSurfaces } from "./surfaceRenderPlan";

const ids = { pluginId: "dash", surfaceId: "board" };

describe("surface icon", () => {
  it("prefers the declared lucide name", () => {
    expect(surfaceIconComponent({ icon: "layout-dashboard", runtime: "browser" }, ids)).toBe(LayoutDashboard);
    expect(surfaceIconComponent({ icon: "Globe", runtime: null }, ids)).toBe(Globe);
  });

  it("falls back to a globe for browser runtimes and a window otherwise", () => {
    expect(surfaceIconComponent({ icon: "unknown-glyph", runtime: "browser" }, ids)).toBe(Globe);
    expect(surfaceIconComponent({ icon: null, runtime: null }, ids)).toBe(AppWindow);
  });

  it("guesses from the ids only until the declaration is known", () => {
    expect(surfaceIconComponent(null, { pluginId: "browser", surfaceId: "page" })).toBe(Globe);
    expect(surfaceIconComponent({ runtime: null }, { pluginId: "browser", surfaceId: "page" })).toBe(AppWindow);
    expect(surfaceIconComponent(null, ids)).toBe(AppWindow);
  });

  it("reads the manifest icon from the declaration list", () => {
    const [withIcon, without] = parseDeclaredSurfaces({
      declared: [
        { plugin_id: "browser", surface_id: "page", renderer: "stream", runtime: "browser", icon: "globe" },
        { plugin_id: "dash", surface_id: "board" },
      ],
    });
    expect(withIcon.icon).toBe("globe");
    expect("icon" in without).toBe(false);
  });
});
