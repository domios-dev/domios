import { afterEach, describe, expect, it, vi } from "vitest";
import type { ApiConfig } from "../../../lib/types";
import { authHeaders, baseUrl } from "../../MemoryPanel/memoryApi";
import {
  fetchTeamMissionControlState,
  type TeamMissionControlState,
} from "./missionControlState";

const API: ApiConfig = {
  enabled: true,
  host: "127.0.0.1",
  port: 7686,
  token: "test-token",
  cors: false,
};

describe("team mission control state adapter", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("reads the team state endpoint with a cursor page", async () => {
    const body = {} as TeamMissionControlState;
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => body,
    });
    vi.stubGlobal("fetch", fetchMock);

    await expect(
      fetchTeamMissionControlState(API, "team/one", { limit: 2, cursor: "next-page" }),
    ).resolves.toBe(body);

    expect(fetchMock).toHaveBeenCalledWith(
      `${baseUrl(API)}/api/teams/team%2Fone/mission-control/state?limit=2&cursor=next-page`,
      {
        headers: authHeaders(API),
        signal: undefined,
      },
    );
  });
});
