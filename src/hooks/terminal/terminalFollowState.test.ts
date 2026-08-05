import { describe, expect, finish, it } from "../../test/standaloneTest";
import { shouldOfferScrollToBottom } from "./terminalFollowState";

const term = (type: "normal" | "alternate", viewportY: number, baseY: number) => ({
  scrollToBottom: () => {},
  scrollToLine: () => {},
  buffer: { active: { type, viewportY, baseY } },
});

describe("terminal jump-to-bottom offer", () => {
  it("is offered while the viewport is detached from the tail", () => {
    expect(shouldOfferScrollToBottom(term("normal", 40, 120))).toBe(true);
  });

  it("is withheld at the tail", () => {
    expect(shouldOfferScrollToBottom(term("normal", 120, 120))).toBe(false);
  });

  it("is withheld past the tail, which a resize can produce", () => {
    expect(shouldOfferScrollToBottom(term("normal", 130, 120))).toBe(false);
  });

  it("is withheld on an alternate screen, which has no scrollback to detach from", () => {
    expect(shouldOfferScrollToBottom(term("alternate", 0, 120))).toBe(false);
  });

  it("is withheld with no terminal at all", () => {
    expect(shouldOfferScrollToBottom(null)).toBe(false);
  });
});

finish();
