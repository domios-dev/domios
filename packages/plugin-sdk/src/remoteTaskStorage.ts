import type {HostFs} from "./host";
import type {RemoteTaskBinding, RemoteTaskStorage} from "./remoteTasks";

type StorageFs = Pick<HostFs, "readFile" | "writeFileAtomic" | "lockFile" | "unlockFile" | "makeDirs">;

export class RemoteTaskFileStorage implements RemoteTaskStorage {
  constructor(private readonly fs: StorageFs, private readonly root: string) {}

  private path(tabId: string): string {
    if (!/^[a-zA-Z0-9_-]+$/.test(tabId)) throw new Error("Invalid remote tab storage identity");
    return `${this.root}/${tabId}.json`;
  }

  read(tabId: string): RemoteTaskBinding | null {
    return this.readFile(this.path(tabId));
  }

  private readFile(path: string): RemoteTaskBinding | null {
    const raw = this.fs.readFile(path);
    if (raw === null) return null;
    const binding = JSON.parse(raw) as RemoteTaskBinding;
    if (!binding || !Number.isSafeInteger(binding.revision) || binding.revision < 0
      || !["creating", "create_uncertain", "bound"].includes(binding.phase)
      || ![binding.providerId, binding.executionMode, binding.backend, binding.environmentId, binding.accountRef, binding.branch].every(value => typeof value === "string" && value.length > 0)
      || (binding.phase === "bound" && (!binding.snapshot || typeof binding.snapshot.taskId !== "string" || !binding.snapshot.taskId))) {
      throw new Error("Stored remote task binding is invalid; creation was not repeated");
    }
    return binding;
  }

  compareExchange(tabId: string, revision: number | null, next: RemoteTaskBinding): boolean {
    const path = this.path(tabId);
    if (!this.fs.makeDirs(this.root)) throw new Error("Remote task storage is unavailable");
    const lock = this.fs.lockFile(`${path}.lock`);
    if (!lock) throw new Error("Remote task storage is busy; creation was not repeated");
    try {
      if ((this.readFile(path)?.revision ?? null) !== revision) return false;
      if (!this.fs.writeFileAtomic(path, JSON.stringify(next))) throw new Error("Remote task binding could not be saved");
      return true;
    } finally {
      this.fs.unlockFile(lock);
    }
  }
}
