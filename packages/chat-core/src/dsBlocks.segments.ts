interface Node<T> { value?: T; left?: Node<T>; right?: Node<T> }
function write<T>(node: Node<T> | undefined, index: number, size: number, value: T): Node<T> {
  if (size === 1) return { value };
  const half = size / 2;
  return index < half
    ? { left: write(node?.left, index, half, value), right: node?.right }
    : { left: node?.left, right: write(node?.right, index - half, half, value) };
}
function read<T>(node: Node<T> | undefined, index: number, size: number): T | undefined {
  if (!node || size === 1) return node?.value;
  const half = size / 2;
  return index < half ? read(node.left, index, half) : read(node.right, index - half, half);
}
function arrayIndex(key: PropertyKey): number | null {
  if (typeof key !== "string" || !/^(?:0|[1-9]\d*)$/.test(key)) return null;
  const index = Number(key);
  return Number.isSafeInteger(index) ? index : null;
}
// Forks share history; writes replace one tree path instead of cloning all committed segments.
export class DsPersistentList<T> {
  readonly view: readonly T[];
  constructor(private root?: Node<T>, private count = 0, private capacity = 1) {
    this.view = new Proxy([] as T[], {
      get: (target, key, receiver) => key === "length" ? this.count : arrayIndex(key) !== null ? this.get(Number(key)) : Reflect.get(target, key, receiver),
      has: (target, key) => arrayIndex(key) !== null ? Number(key) < this.count : Reflect.has(target, key),
      ownKeys: () => [...Array.from({ length: this.count }, (_, index) => String(index)), "length"],
      getOwnPropertyDescriptor: (target, key) => {
        if (key === "length") return { value: this.count, writable: true, enumerable: false, configurable: false };
        const index = arrayIndex(key);
        if (index !== null) return index < this.count ? { value: this.get(index), writable: false, enumerable: true, configurable: true } : undefined;
        return Reflect.getOwnPropertyDescriptor(target, key);
      },
      set: () => false,
      deleteProperty: () => false,
      defineProperty: () => false,
    });
  }
  get length(): number { return this.count; }
  set length(value: number) {
    if (!Number.isInteger(value) || value < 0 || value > this.count) throw new Error("Invalid persistent list truncation");
    this.count = value;
  }
  get(index: number): T | undefined { return index >= 0 && index < this.count ? read(this.root, index, this.capacity) : undefined; }
  push(value: T): void {
    if (this.count === this.capacity) { this.root = { left: this.root }; this.capacity *= 2; }
    this.root = write(this.root, this.count++, this.capacity, value);
  }
  pop(): T | undefined {
    const value = this.get(this.count - 1);
    if (this.count) this.count--;
    return value;
  }
  snapshot(): readonly T[] { return this.fork().view; }
  fork(): DsPersistentList<T> { return new DsPersistentList(this.root, this.count, this.capacity); }
}
