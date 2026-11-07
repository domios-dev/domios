import { expect, it } from "vitest";
import { NOTICE_SUMMARY_MAX_CHARS, boundedNotice } from "./boundedNotice";

it("keeps a short single-sentence notice whole with no details", () => {
  expect(boundedNotice("  Model opaque has been deprecated  ")).toEqual({ summary: "Model opaque has been deprecated", details: null });
});

it("summarises a multi-line provider error by its first worded sentence and keeps the raw text", () => {
  const raw = [
    "{",
    "Opaque quota exceeded for metric opaque/requests. Please retry in 30s.",
    "  https://opaque.example/v1/quota?metric=opaque",
    "  quotaValue: 250",
  ].join("\n");
  expect(boundedNotice(raw)).toEqual({ summary: "Opaque quota exceeded for metric opaque/requests.", details: raw });
});

it("bounds an unbroken line and never splits on a dot inside a URL", () => {
  const raw = `Error fetching from https://opaque.example/v1/models:${"x".repeat(400)}`;
  const notice = boundedNotice(raw);
  expect(Array.from(notice.summary)).toHaveLength(NOTICE_SUMMARY_MAX_CHARS);
  expect(notice.summary.startsWith("Error fetching from https://opaque.example/v1/models:")).toBe(true);
  expect(notice.details).toBe(raw);
});
