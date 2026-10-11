import { describe, expect, it } from "vitest";
import type { Notification } from "../../lib/types";
import { attentionSurfaceLayout, isLiveAttention, retainedAttention } from "./attentionSurfaceModel";

const request: Notification = {
  id: "ask", tab_id: "tab", workspace_id: null, title: "opaque", body: "/long/path\n".repeat(100),
  severity: "Info", level: "action_needed", timestamp: 1, read: true,
  answer: { asked_from_tab: "tab", answered: null, delivered: false },
};

describe("attention surface", () => {
  it("keeps details collapsed regardless of content", () => {
    expect(attentionSurfaceLayout([], 1000).showDetails).toBe(false);
    expect(attentionSurfaceLayout([], 1000, true).showDetails).toBe(true);
  });
  it("offers inline actions only within the available width", () => {
    expect(attentionSurfaceLayout(["Yes", "No"], 500).inlineActions).toBe(true);
    expect(attentionSurfaceLayout(["Yes", "No"], 200).showChoose).toBe(true);
    expect(attentionSurfaceLayout(["x".repeat(100)], 500).showChoose).toBe(true);
    expect(attentionSurfaceLayout([], 100).showChoose).toBe(false);
  });
  it("retains read requests while they remain unanswered", () => {
    expect(retainedAttention([request], request.id)).toBe(request);
  });
  it.each([true, false])("removes answers independently of delivery (%s)", (delivered) => {
    const answered = { ...request, answer: { ...request.answer!, answered: "Yes", delivered } };
    expect(isLiveAttention(answered)).toBe(false);
    expect(retainedAttention([answered], request.id)).toBeUndefined();
  });
  it("removes resolved, cleared, and orphaned requests", () => {
    expect(retainedAttention([{ ...request, resolved: true }], request.id)).toBeUndefined();
    expect(retainedAttention([], request.id)).toBeUndefined();
    expect(retainedAttention([{ ...request, answer: { ...request.answer!, asked_from_tab: null } }], request.id)).toBeUndefined();
  });
});
