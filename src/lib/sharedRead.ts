export class SharedRead<T> {
  value: T | undefined;
  private flight: Promise<T> | undefined;
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

  read(load: () => Promise<T>, force = false): Promise<T> {
    if (this.flight) return this.flight;
    if (!force && this.now() - this.attemptedAt < 60_000) {
      return this.failure === undefined ? Promise.resolve(this.value!) : Promise.reject(this.failure);
    }
    this.attemptedAt = this.now();
    const revision = this.revision;
    this.flight = load().then(value => {
      if (this.revision === revision) this.write(value);
      return this.value!;
    }, failure => {
      if (this.revision !== revision) return this.value!;
      this.failure = failure;
      throw failure;
    }).finally(() => { this.flight = undefined; });
    return this.flight;
  }
}
