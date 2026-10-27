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
