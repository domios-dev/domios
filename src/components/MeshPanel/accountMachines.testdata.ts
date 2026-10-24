import type {
  AccountMachineRow,
  AccountMeshClient,
  AccountMeshOutcome,
  AccountMeshSnapshot,
  LocalPairFact,
  MeshTarget,
} from "./accountMeshClient";

export const NOW = 1_800_000_000;

const key = (seed: string) => seed.repeat(64).slice(0, 64);

export function machine(installationId: string, name: string, patch: Partial<AccountMachineRow> = {}): AccountMachineRow {
  return {
    installation_id: installationId,
    key_id: key(installationId.slice(-1)),
    key_generation: 2,
    slot_revision: 7,
    active: true,
    presence: "present",
    admitted_at: NOW - 3600,
    authenticated_at: NOW - 60,
    display_name: name,
    metadata_revision: 4,
    publication: "complete",
    membership_expires_at: NOW + 1800,
    eligibility: "eligible",
    endpoint: { tailnet_ip: "100.64.0.9", api_port: 7685, expires_at: NOW + 600 },
    ...patch,
  };
}

export const selfRow = machine("inst-self-a", "Studio PC", { slot_revision: 3, metadata_revision: 2, key_generation: 1 });
export const macRow = machine("inst-mac-b", "MacBook");
export const lenovoRow = machine("inst-lenovo-c", "Lenovo");
export const airRow = machine("inst-air-d", "Air", { endpoint: null });

export function pair(instanceId: string, name: string, patch: Partial<LocalPairFact> = {}): LocalPairFact {
  return { instance_id: instanceId, installation_id: null, admission: "adopted", online: true, name, ...patch };
}

export function snapshot(patch: Partial<AccountMeshSnapshot> = {}): AccountMeshSnapshot {
  return {
    binding: "current",
    traffic: "open",
    cleanup: "none",
    self: {
      installation_id: selfRow.installation_id,
      key_id: selfRow.key_id,
      key_generation: selfRow.key_generation,
      slot_revision: selfRow.slot_revision,
      metadata_revision: selfRow.metadata_revision,
      display_name: selfRow.display_name,
    },
    registry: { observed_at: NOW, rows: [selfRow, macRow, lenovoRow] },
    registry_failure: null,
    enrollment: "enrolled",
    last_refusal: null,
    pairs: [],
    ...patch,
  };
}

export function targetOf(row: MeshTarget): MeshTarget {
  return { installation_id: row.installation_id, key_id: row.key_id, key_generation: row.key_generation, slot_revision: row.slot_revision };
}

interface Deferred<T> { promise: Promise<T>; resolve: (value: T) => void; reject: (error: unknown) => void }
export function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

export class FakeAccountMeshClient implements AccountMeshClient {
  state: AccountMeshSnapshot;
  snapshotCalls = 0;
  listCalls = 0;
  listRows: AccountMachineRow[] | null = null;
  listOutcome: AccountMeshOutcome = { ok: true };
  enrollCalls = 0;
  enrollOutcome: AccountMeshOutcome = { ok: true };
  linkCalls: MeshTarget[] = [];
  revokeCalls: MeshTarget[] = [];
  renameCalls: Array<[number, string]> = [];
  peerRenameCalls: Array<[string, number, string]> = [];
  unpairCalls: string[] = [];
  linkOutcome = deferred<void>();
  revokeOutcomes: AccountMeshOutcome[] = [];
  renameOutcomes: AccountMeshOutcome[] = [];
  unpairFailure: Error | null = null;
  private listeners = new Set<() => void>();

  constructor(state: AccountMeshSnapshot) {
    this.state = state;
  }

  async snapshot(): Promise<AccountMeshSnapshot> {
    this.snapshotCalls++;
    return structuredClone(this.state);
  }

  async list(): Promise<AccountMeshOutcome> {
    this.listCalls++;
    if (this.listRows !== null) {
      this.state = { ...this.state, registry: { observed_at: NOW, rows: this.listRows }, registry_failure: null };
    }
    return this.listOutcome;
  }

  async enroll(): Promise<AccountMeshOutcome> {
    this.enrollCalls++;
    return this.enrollOutcome;
  }

  subscribe(onChange: () => void): () => void {
    this.listeners.add(onChange);
    return () => { this.listeners.delete(onChange); };
  }

  emit(state: AccountMeshSnapshot) {
    this.state = state;
    for (const listener of this.listeners) listener();
  }

  link(target: MeshTarget): Promise<void> {
    this.linkCalls.push(target);
    return this.linkOutcome.promise;
  }

  async renameSelf(expectedMetadataRevision: number, displayName: string): Promise<AccountMeshOutcome> {
    this.renameCalls.push([expectedMetadataRevision, displayName]);
    return this.renameOutcomes.shift() ?? { ok: true };
  }

  async renamePeer(instanceId: string, expectedMetadataRevision: number, displayName: string): Promise<AccountMeshOutcome> {
    this.peerRenameCalls.push([instanceId, expectedMetadataRevision, displayName]);
    return this.renameOutcomes.shift() ?? { ok: true };
  }

  async revoke(target: MeshTarget): Promise<AccountMeshOutcome> {
    this.revokeCalls.push(target);
    return this.revokeOutcomes.shift() ?? { ok: true };
  }

  async unpair(instanceId: string): Promise<void> {
    this.unpairCalls.push(instanceId);
    if (this.unpairFailure) throw this.unpairFailure;
  }

  acknowledgedLimits: string[] = [];

  async acknowledgeLimit(requestId: string): Promise<void> {
    this.acknowledgedLimits.push(requestId);
  }
}
