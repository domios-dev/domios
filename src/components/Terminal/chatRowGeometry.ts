import { CHAT_ROW_ESTIMATE } from "./chatMountedWindow";

export interface ChatRowGeometry {
  readonly count: number;
  offset(index: number): number;
  boundary(top: number, inclusive: boolean): number;
}

export class IndexedChatRowGeometry implements ChatRowGeometry {
  private heights: number[] = [];
  private sums: number[] = [0];

  constructor(heights: readonly number[] = []) {
    for (const height of heights) this.append(height);
  }

  get count(): number { return this.heights.length; }

  offset(index: number): number {
    let sum = 0;
    for (let cursor = index; cursor > 0; cursor -= cursor & -cursor) sum += this.sums[cursor];
    return sum;
  }

  append(height: number): void {
    const value = Number.isFinite(height) && height >= 0 ? height : CHAT_ROW_ESTIMATE;
    const index = this.count + 1;
    const start = index - (index & -index);
    this.sums.push(this.offset(index - 1) - this.offset(start) + value);
    this.heights.push(value);
  }

  update(index: number, height: number): boolean {
    if (index < 0 || index >= this.count || !Number.isFinite(height) || height < 0 || height === this.heights[index]) return false;
    const delta = height - this.heights[index];
    this.heights[index] = height;
    for (let cursor = index + 1; cursor <= this.count; cursor += cursor & -cursor) this.sums[cursor] += delta;
    return true;
  }

  boundary(top: number, inclusive: boolean): number {
    let index = 0;
    let sum = 0;
    for (let step = 2 ** Math.floor(Math.log2(Math.max(1, this.count))); step >= 1; step /= 2) {
      const next = index + step;
      if (next > this.count) continue;
      const value = sum + this.sums[next];
      if (inclusive ? value <= top : value < top) { index = next; sum = value; }
    }
    return index;
  }
}
