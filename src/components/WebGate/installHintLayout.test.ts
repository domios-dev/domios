import { describe, expect, it } from "vitest";
import { installHintLayout } from "./installHintLayout";

const bounds = { touch: true, viewportTop: 0, viewportHeight: 844, titlebarBottom: 44, composerTop: 730 };

describe("installation hint layout", () => {
  it.each([[844, 730], [390, 276], [450, 330]])("fits above the composer in a %i px viewport", (viewportHeight, composerTop) => {
    const layout = installHintLayout({ ...bounds, viewportHeight, composerTop });
    expect(layout.top).toBe(50);
    expect(layout.top! + layout.maxHeight!).toBeLessThan(composerTop);
  });
  it("follows the visual viewport and titlebar safe area", () => {
    expect(installHintLayout({ ...bounds, viewportTop: 60, viewportHeight: 390, titlebarBottom: 44, composerTop: 350 })).toMatchObject({ top: 66, maxHeight: 278 });
  });
  it("fits the viewport when there is no composer", () => {
    expect(installHintLayout({ ...bounds, composerTop: null })).toMatchObject({ top: 50, maxHeight: 788 });
  });
  it("hides when the working surface leaves no room", () => {
    expect(installHintLayout({ ...bounds, composerTop: 80 }).display).toBe("none");
  });
  it("preserves desktop placement", () => {
    expect(installHintLayout({ ...bounds, touch: false })).toEqual({});
  });
});
