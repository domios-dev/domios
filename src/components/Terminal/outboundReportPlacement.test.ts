import { describe, expect, finish, it } from "../../test/standaloneTest";
import { messageTimeMs, normalizeEpochMs, placeOutboundReports } from "./outboundReportPlacement";

const at = (ms: number | null) => ({ at: ms });
const timeOf = (m: { at: number | null }) => m.at;

describe("outbound report placement", () => {
  it("a report with newer messages after it is placed before them, not pinned to the bottom", () => {
    const messages = [at(100), at(200), at(300)];
    const { afterIndex, trailing } = placeOutboundReports(messages, timeOf, [250]);
    expect(afterIndex.get(1)).toEqual([0]);
    expect(trailing).toEqual([]);
  });

  it("a report newer than every message trails, because nothing exists to place it before", () => {
    const messages = [at(100), at(200)];
    const { afterIndex, trailing } = placeOutboundReports(messages, timeOf, [900]);
    expect(afterIndex.size).toBe(0);
    expect(trailing).toEqual([0]);
  });

  it("a report older than every message is placed before the first", () => {
    const messages = [at(500), at(600)];
    const { afterIndex } = placeOutboundReports(messages, timeOf, [100]);
    expect(afterIndex.get(-1)).toEqual([0]);
  });

  it("an untimed report never reorders anything", () => {
    const messages = [at(100), at(200)];
    const { afterIndex, trailing } = placeOutboundReports(messages, timeOf, [null]);
    expect(afterIndex.size).toBe(0);
    expect(trailing).toEqual([0]);
  });

  it("untimed messages are skipped as anchors rather than treated as time zero", () => {
    const messages = [at(100), at(null), at(300)];
    const { afterIndex } = placeOutboundReports(messages, timeOf, [150]);
    expect(afterIndex.get(0)).toEqual([0]);
  });

  it("several reports keep their relative order inside one slot", () => {
    const messages = [at(100), at(400)];
    const { afterIndex } = placeOutboundReports(messages, timeOf, [200, 300]);
    expect(afterIndex.get(0)).toEqual([0, 1]);
  });

  // The structural cause of issue 443: with nothing to place against, every
  // stamped report trails, so the whole rail lands above the loading view.
  it("against a zero-length message list every stamped report falls into trailing", () => {
    const { afterIndex, trailing } = placeOutboundReports<{ at: number | null }>([], timeOf, [100, 200, 300]);
    expect(afterIndex.size).toBe(0);
    expect(trailing).toEqual([0, 1, 2]);
  });

  it("ISO timestamps convert and unparseable ones read as no position", () => {
    expect(messageTimeMs("1970-01-01T00:00:01.000Z")).toBe(1000);
    expect(messageTimeMs(undefined)).toBe(null);
    expect(messageTimeMs("not a date")).toBe(null);
  });
});

describe("report timestamp unit normalization", () => {
  const isoWindow = [
    { at: messageTimeMs("2026-08-08T10:00:00.000Z") },
    { at: messageTimeMs("2026-08-08T10:05:00.000Z") },
    { at: messageTimeMs("2026-08-08T10:10:00.000Z") },
  ];

  it("a report the daemon stamped in epoch-seconds places after the ISO message that predates it, not above the window", () => {
    const reportEpochSeconds = Date.parse("2026-08-08T10:07:00.000Z") / 1000;
    const { afterIndex, trailing } = placeOutboundReports(
      isoWindow,
      timeOf,
      [normalizeEpochMs(reportEpochSeconds)],
    );
    expect(afterIndex.get(-1)).toBeUndefined();
    expect(afterIndex.get(1)).toEqual([0]);
    expect(trailing).toEqual([]);
  });

  it("a report genuinely older than the whole window still places before the first message", () => {
    const oldReportEpochSeconds = Date.parse("2020-01-01T00:00:00.000Z") / 1000;
    const { afterIndex, trailing } = placeOutboundReports(
      isoWindow,
      timeOf,
      [normalizeEpochMs(oldReportEpochSeconds)],
    );
    expect(afterIndex.get(-1)).toEqual([0]);
    expect(trailing).toEqual([]);
  });

  it("an already-milliseconds timestamp passes through unchanged", () => {
    const reportEpochMs = Date.parse("2026-08-08T10:07:00.000Z");
    expect(normalizeEpochMs(reportEpochMs)).toBe(reportEpochMs);
    const { afterIndex } = placeOutboundReports(isoWindow, timeOf, [normalizeEpochMs(reportEpochMs)]);
    expect(afterIndex.get(1)).toEqual([0]);
  });

  it("seconds convert once and non-numbers read as no position", () => {
    expect(normalizeEpochMs(1_770_000_000)).toBe(1_770_000_000_000);
    expect(normalizeEpochMs(null)).toBe(null);
    expect(normalizeEpochMs(undefined)).toBe(null);
    expect(normalizeEpochMs(Number.NaN)).toBe(null);
    expect(normalizeEpochMs("1770000000")).toBe(null);
  });
});

finish();
