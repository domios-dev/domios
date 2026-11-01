import { beforeEach, describe, expect, it, vi } from "vitest";

const raised = vi.hoisted(() => [] as unknown[]);
vi.mock("../billing/accountMeshLimit", () => ({
  raiseMeshMachineLimit: (notice: unknown) => {
    raised.push(notice);
    return true;
  },
}));

import type { AccountMeshClient, AccountMeshSnapshot } from "./accountMeshClient";
import { deliverLimitNotice, pendingLimitNotice } from "./accountMeshLimitNotice";

function client(snapshot: Partial<AccountMeshSnapshot>) {
  const acknowledged: string[] = [];
  const fake = {
    snapshot: async () => snapshot as AccountMeshSnapshot,
    enroll: async () => ({ ok: true as const }),
    list: async () => ({ ok: true as const }),
    subscribe: () => () => undefined,
    renameSelf: async () => ({ ok: true as const }),
    revokeSession: async () => ({ ok: true as const }),
    revoke: async () => ({ ok: true as const }),
    acknowledgeLimit: async (requestId: string) => {
      acknowledged.push(requestId);
    },
  } satisfies AccountMeshClient;
  return { fake, acknowledged };
}

describe("account mesh limit notice", () => {
  beforeEach(() => {
    raised.length = 0;
  });

  it("raises the held refusal with its request id, then acknowledges it", async () => {
    const refusal = { reason: "mesh.machines", limit: 2, active_count: 2 };
    const { fake, acknowledged } = client({
      limit_notice: { request_id: "req-1", refusal, trigger: "automatic" },
    });

    expect(await deliverLimitNotice(fake)).toBe(true);
    expect(raised).toEqual([{ requestId: "req-1", refusal }]);
    expect(acknowledged).toEqual(["req-1"]);
  });

  it("does nothing without a held notice", async () => {
    const { fake, acknowledged } = client({ limit_notice: null });

    expect(await deliverLimitNotice(fake)).toBe(false);
    expect(raised).toEqual([]);
    expect(acknowledged).toEqual([]);
  });

  it("ignores a notice without a request id", () => {
    expect(
      pendingLimitNotice({ limit_notice: { request_id: "", refusal: {}, trigger: "automatic" } }),
    ).toBeNull();
    expect(pendingLimitNotice(undefined)).toBeNull();
  });
});
