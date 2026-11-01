import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { AccountMeshClientProvider, type AccountMeshSnapshot } from "../MeshPanel/accountMeshClient";
import { deferred, FakeAccountMeshClient, snapshot } from "../MeshPanel/accountMachines.testdata";
import { useAccountEnrollmentFailure, useEnrollmentNotice } from "./useAccountEnrollmentFailure";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | undefined;
let host: HTMLDivElement | undefined;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = undefined;
  host = undefined;
});

function Surface({ identity }: { identity: string | null }) {
  const failure = useAccountEnrollmentFailure(identity);
  return <output>{failure?.reason ?? "clear"}</output>;
}

async function render(client: FakeAccountMeshClient, identity: string | null = "owner") {
  if (!root) {
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  }
  await act(async () => root!.render(<AccountMeshClientProvider value={client}><Surface identity={identity} /></AccountMeshClientProvider>));
}

it("loads the enrollment refusal and clears it on an enrollment event without retrying", async () => {
  const client = new FakeAccountMeshClient(snapshot({ enrollment: "refused", last_refusal: { reason: "proof_invalid" } }));
  await render(client);
  expect(host?.textContent).toBe("proof_invalid");
  await act(async () => client.emit(snapshot()));
  expect(host?.textContent).toBe("clear");
  expect(client.enrollCalls).toBe(0);
  expect(client.listCalls).toBe(0);
});

it("discards an old account's pending snapshot after signing out", async () => {
  const client = new FakeAccountMeshClient(snapshot());
  const pending = deferred<AccountMeshSnapshot>();
  const read = vi.spyOn(client, "snapshot").mockReturnValue(pending.promise);
  await render(client);
  await render(client, null);
  await act(async () => pending.resolve(snapshot({ enrollment: "refused", last_refusal: { reason: "key_conflict" } })));
  expect(host?.textContent).toBe("clear");
  const calls = read.mock.calls.length;
  await act(async () => client.emit(snapshot()));
  expect(read.mock.calls).toHaveLength(calls);
});

function Notice({ identity }: { identity: string | null }) {
  return <output>{useEnrollmentNotice(identity) ?? "clear"}</output>;
}

it("shows the deadline in the account menu observation and clears it without enrolling", async () => {
  const now = vi.spyOn(Date, "now").mockReturnValue(1_800_000_000_000);
  try {
    const state = snapshot({ enrollment: "refused", last_refusal: { reason: "enrollment_budget", retry_after_seconds: 121 }, retry_at: 1_800_000_121 });
    const client = new FakeAccountMeshClient(state);
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    const paint = async (identity: string | null) => {
      await act(async () => root!.render(<AccountMeshClientProvider value={client}><Notice identity={identity} /></AccountMeshClientProvider>));
    };
    await paint("owner");
    expect(host.textContent).toContain("Try again in 3 min");
    now.mockReturnValue(1_800_000_061_000);
    await act(async () => client.emit(state));
    expect(host.textContent).toContain("Try again in 1 min");
    await paint(null);
    expect(host.textContent).toBe("clear");
    expect(client.enrollCalls).toBe(0);
    expect(client.listCalls).toBe(0);
  } finally { now.mockRestore(); }
});
