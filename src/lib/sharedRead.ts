export class SharedRead<T> {
  value: T | undefined;
  private flight: Promise<T> | undefined;
  private queued: (() => Promise<T>) | undefined;
  private attemptedAt = -Infinity;
  private failure: unknown;
  private revision = 0;
  private readonly listeners = new Set<(value: T) => void>();

  constructor(private readonly now = () => Date.now()) {}

  subscribe(listener: (value: T) => void) {
    this.listeners.add(listener);
    return () => { this.listeners.delete(listener); };
  }

  write(value: T) {
    this.revision++;
    this.value = value;
    this.failure = undefined;
    this.attemptedAt = this.now();
    for (const listener of this.listeners) listener(value);
  }

  refresh(load: () => Promise<T>): Promise<T> {
    return this.flight ?? this.read(load, true);
  }

  read(load: () => Promise<T>, force = false): Promise<T> {
    if (this.flight) {
      if (force) {
        if (!this.queued) this.revision++;
        this.queued = load;
      }
      return this.flight;
    }
    if (!force && this.now() - this.attemptedAt < 60_000) {
      return this.failure === undefined ? Promise.resolve(this.value!) : Promise.reject(this.failure);
    }
    let complete!: (value: T) => void;
    let fail!: (failure: unknown) => void;
    const flight = new Promise<T>((resolve, reject) => { complete = resolve; fail = reject; });
    this.flight = flight;
    void this.fetch(load).then(complete, fail);
    return flight;
  }

  private async fetch(load: () => Promise<T>): Promise<T> {
    try {
      for (;;) {
        this.attemptedAt = this.now();
        const revision = this.revision;
        try {
          const value = await load();
          if (!this.queued) {
            if (this.revision === revision) this.write(value);
            if (!this.queued) return this.value!;
          }
        } catch (failure: unknown) {
          if (!this.queued) {
            if (this.revision !== revision) return this.value!;
            this.failure = failure;
            throw failure;
          }
        }
        load = this.queued!;
        this.queued = undefined;
      }
    } finally {
      this.flight = undefined;
    }
  }
}
