import { describe, expect, finish, it } from "../../test/standaloneTest";
import { messageTimeMs, placeOutboundReports } from "./outboundReportPlacement";

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

  it("ISO timestamps convert and unparseable ones read as no position", () => {
    expect(messageTimeMs("1970-01-01T00:00:01.000Z")).toBe(1000);
    expect(messageTimeMs(undefined)).toBe(null);
    expect(messageTimeMs("not a date")).toBe(null);
  });
});

finish();
