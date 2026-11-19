import { describe, expect, it } from "vitest";
import { cronRunTranscriptRequest } from "./cronRunTranscriptRequest";

describe("cronRunTranscriptRequest", () => {
  it("uses the run route and owner without consulting a changed job provider", () => {
    expect(cronRunTranscriptRequest("cron/old", {
      run_id: "run 1",
      owner_instance_id: "machine-b",
    })).toEqual({
      targetInstanceId: "machine-b",
      path: "/api/crons/cron%2Fold/history/run%201",
    });
  });

  it("uses the source peer when an older remote run has no recorded owner", () => {
    expect(cronRunTranscriptRequest("cron-1", {
      run_id: "run-1",
      owner_instance_id: null,
    }, "machine-air")).toEqual({
      targetInstanceId: "machine-air",
      path: "/api/crons/cron-1/history/run-1",
    });
  });
});
