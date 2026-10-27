import { expect, it } from "vitest";
import { SharedRead } from "./sharedRead";

it("shares a flight and enforces a minute between automatic attempts", async () => {
  let now = 0;
  const read = new SharedRead<number>(() => now);
  let finish!: (value: number) => void;
  let calls = 0;
  const load = () => { calls++; return new Promise<number>(resolve => { finish = resolve; }); };
  const first = read.read(load);
  expect(read.read(load)).toBe(first);
  finish(1);
  expect(await first).toBe(1);
  now = 59_999;
  expect(await read.read(load)).toBe(1);
  expect(calls).toBe(1);
  now = 60_000;
  const next = read.read(load);
  finish(2);
  expect(await next).toBe(2);
  const explicit = read.read(load, true);
  finish(3);
  expect(await explicit).toBe(3);
  expect(calls).toBe(3);
});

it("keeps a pushed value when an older flight completes or fails", async () => {
  for (const reject of [false, true]) {
    const read = new SharedRead<number>();
    let finish!: (value: number) => void;
    let fail!: (failure: Error) => void;
    const flight = read.read(() => new Promise<number>((resolve, reject) => { finish = resolve; fail = reject; }));
    read.write(9);
    if (reject) fail(new Error("offline")); else finish(1);
    expect(await flight).toBe(9);
    expect(read.value).toBe(9);
  }
});

it("does not turn failed focus requests into a retry storm", async () => {
  let now = 0;
  let calls = 0;
  const read = new SharedRead<number>(() => now);
  const load = async () => { calls++; throw new Error("offline"); };
  await expect(read.read(load)).rejects.toThrow("offline");
  now = 59_999;
  await expect(read.read(load)).rejects.toThrow("offline");
  expect(calls).toBe(1);
  now = 60_000;
  await expect(read.read(load)).rejects.toThrow("offline");
  expect(calls).toBe(2);
});

it.each([false, true])("queues one fresh shared read after invalidating an old flight (failure: %s)", async reject => {
  const read = new SharedRead<number>();
  const published: number[] = [];
  read.subscribe(value => { published.push(value); });
  let finish!: (value: number) => void;
  let fail!: (failure: Error) => void;
  let calls = 0;
  const old = read.read(() => { calls++; return new Promise<number>((resolve, reject) => { finish = resolve; fail = reject; }); });
  const fresh = async () => { calls++; return 2; };
  const forced = read.read(fresh, true);
  expect(read.read(fresh, true)).toBe(forced);
  expect(read.read(fresh, true)).toBe(forced);
  expect(read.read(fresh)).toBe(forced);
  expect(calls).toBe(1);
  if (reject) fail(new Error("old failure")); else finish(1);
  expect(await old).toBe(2);
  expect(await forced).toBe(2);
  expect(calls).toBe(2);
  expect(published).toEqual([2]);
  expect(await read.read(fresh)).toBe(2);
  expect(calls).toBe(2);
});

it("keeps only the latest queued load when another mutation occurs during a replacement flight", async () => {
  const read = new SharedRead<number>();
  const published: number[] = [];
  read.subscribe(value => { published.push(value); });
  let finishOld!: (value: number) => void;
  let finishReplacement!: (value: number) => void;
  let replacementStarted!: () => void;
  const started = new Promise<void>(resolve => { replacementStarted = resolve; });
  let calls = 0;
  const flight = read.read(() => { calls++; return new Promise<number>(resolve => { finishOld = resolve; }); });
  read.read(() => {
    calls++;
    replacementStarted();
    return new Promise<number>(resolve => { finishReplacement = resolve; });
  }, true);
  finishOld(1);
  await started;
  read.read(async () => { calls++; return 3; }, true);
  read.read(async () => { calls++; return 4; }, true);
  expect(calls).toBe(2);
  finishReplacement(2);
  expect(await flight).toBe(4);
  expect(calls).toBe(3);
  expect(published).toEqual([4]);
});

it("starts a forced read even while the previous caller's promise is settling", async () => {
  const read = new SharedRead<number>();
  const first = read.read(async () => 1);
  await Promise.resolve();
  expect(await read.read(async () => 2, true)).toBe(2);
  await first;
  expect(read.value).toBe(2);
});
