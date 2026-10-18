export interface RemoteTaskCapability {
  backend: string;
  mode: string;
  displayName: string;
  environments: RemoteEnvironment[];
  availability: {status: "unavailable"; reason: string} | {status: "available"};
  followUp: boolean;
  cancel: boolean;
  applyDiff: boolean;
}

export interface RemoteEnvironment {
  id: string;
  label: string;
  accountRef: string;
}

export type RemoteTaskStatus = "pending" | "running" | "ready" | "error";

export interface RemoteTaskSnapshot {
  taskId: string;
  attempt: string | null;
  status: RemoteTaskStatus;
  result: string;
  diff: string | null;
  cursor: string | null;
  error: string | null;
}

export interface RemoteTaskBinding {
  revision: number;
  providerId: string;
  executionMode: string;
  backend: string;
  environmentId: string;
  accountRef: string;
  branch: string;
  phase: "creating" | "create_uncertain" | "bound";
  snapshot: RemoteTaskSnapshot | null;
}

export interface RemoteTaskStorage {
  read(tabId: string): RemoteTaskBinding | null;
  compareExchange(tabId: string, revision: number | null, next: RemoteTaskBinding): boolean;
}

export interface RemoteTaskBackend {
  capability: RemoteTaskCapability;
  environments(): RemoteEnvironment[];
  create(input: {environmentId: string; accountRef: string; branch: string; prompt: string}): {taskId: string};
  read(binding: RemoteTaskBinding): RemoteTaskSnapshot;
}

export class RemoteTaskController {
  constructor(private readonly storage: RemoteTaskStorage, private readonly backend: RemoteTaskBackend) {}

  create(tabId: string, providerId: string, environment: RemoteEnvironment, branch: string, prompt: string): RemoteTaskBinding {
    if (this.backend.capability.availability.status !== "available") {
      throw new Error(this.backend.capability.availability.reason);
    }
    if (!branch.trim() || !prompt.trim()) throw new Error("A branch and task are required");
    const environments = this.backend.environments();
    if (!environments.some(row => row.id === environment.id && row.accountRef === environment.accountRef)) {
      throw new Error("Choose an environment available to this account");
    }
    const claim: RemoteTaskBinding = {
      revision: 0, providerId, executionMode: this.backend.capability.mode, backend: this.backend.capability.backend,
      environmentId: environment.id, accountRef: environment.accountRef, branch,
      phase: "creating", snapshot: null,
    };
    if (!this.storage.compareExchange(tabId, null, claim)) {
      throw new Error("This tab already has a remote task; creation was not repeated");
    }
    let acknowledgement: {taskId: string};
    try {
      acknowledgement = this.backend.create({environmentId: environment.id, accountRef: environment.accountRef, branch, prompt});
      if (!acknowledgement.taskId) throw new Error("Missing remote task acknowledgement");
    } catch {
      const uncertain: RemoteTaskBinding = {...claim, revision: 1, phase: "create_uncertain"};
      this.storage.compareExchange(tabId, claim.revision, uncertain);
      return uncertain;
    }
    const bound: RemoteTaskBinding = {
      ...claim, revision: 1, phase: "bound",
      snapshot: {taskId: acknowledgement.taskId, attempt: null, status: "pending", result: "", diff: null, cursor: null, error: null},
    };
    if (!this.storage.compareExchange(tabId, claim.revision, bound)) {
      throw new Error("Remote creation was acknowledged but its binding could not be saved; do not submit again");
    }
    return bound;
  }

  restore(tabId: string): RemoteTaskBinding | null {
    const binding = this.storage.read(tabId);
    if (!binding) return null;
    if (binding.executionMode !== this.backend.capability.mode) throw new Error("Remote execution mode does not match the stored tab");
    if (binding.backend !== this.backend.capability.backend) throw new Error("Remote backend does not match the stored tab");
    if (binding.phase !== "bound" || !binding.snapshot) {
      if (binding.phase === "create_uncertain") return binding;
      const uncertain: RemoteTaskBinding = {...binding, revision: binding.revision + 1, phase: "create_uncertain"};
      if (!this.storage.compareExchange(tabId, binding.revision, uncertain)) return this.storage.read(tabId);
      return uncertain;
    }
    const snapshot = this.backend.read(binding);
    if (snapshot.taskId !== binding.snapshot.taskId) throw new Error("Remote response belongs to a different task");
    const updated: RemoteTaskBinding = {...binding, revision: binding.revision + 1, snapshot};
    if (!this.storage.compareExchange(tabId, binding.revision, updated)) return this.storage.read(tabId);
    return updated;
  }
}
