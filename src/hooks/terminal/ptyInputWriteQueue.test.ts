import { describe, expect, finish, it } from "../../test/standaloneTest";
import { PtyInputWriteQueue } from "./ptyInputWriteQueue";

describe("PTY input write queue", () => {
  it("serializes writes and coalesces keys queued behind an in-flight write", async () => {
    const writes: string[] = [];
    let releaseFirst!: () => void;
    const firstWrite = new Promise<void>((resolve) => {
      releaseFirst = resolve;
    });
    const queue = new PtyInputWriteQueue(async (_ptyId, data) => {
      writes.push(data);
      if (writes.length === 1) await firstWrite;
    }, () => {});

    const drained = queue.enqueue("pty-a", "a");
    queue.enqueue("pty-a", "b");
    queue.enqueue("pty-a", "c");

    expect(writes).toEqual(["a"]);
    releaseFirst();
    await drained;
    expect(writes).toEqual(["a", "bc"]);
  });

  it("keeps independent PTYs from blocking each other", async () => {
    const writes: string[] = [];
    let releaseFirst!: () => void;
    const firstWrite = new Promise<void>((resolve) => {
      releaseFirst = resolve;
    });
    const queue = new PtyInputWriteQueue(async (ptyId, data) => {
      writes.push(`${ptyId}:${data}`);
      if (ptyId === "pty-a") await firstWrite;
    }, () => {});

    const first = queue.enqueue("pty-a", "a");
    await queue.enqueue("pty-b", "b");
    expect(writes).toEqual(["pty-a:a", "pty-b:b"]);
    releaseFirst();
    await first;
  });

  it("reports a failed batch and continues with later queued keys", async () => {
    const writes: string[] = [];
    const failures: Array<{ ptyId: string; dataLength: number }> = [];
    let releaseFirst!: () => void;
    let startedFirst!: () => void;
    const firstStarted = new Promise<void>((resolve) => {
      startedFirst = resolve;
    });
    const releaseWrite = new Promise<void>((resolve) => {
      releaseFirst = resolve;
    });
    const queue = new PtyInputWriteQueue(async (_ptyId, data) => {
      writes.push(data);
      if (data === "a") {
        startedFirst();
        await releaseWrite;
        throw new Error("route unavailable");
      }
    }, (ptyId, dataLength) => failures.push({ ptyId, dataLength }));

    const drained = queue.enqueue("pty-a", "a");
    await firstStarted;
    queue.enqueue("pty-a", "b");
    releaseFirst();
    await drained;

    expect(writes).toEqual(["a", "b"]);
    expect(failures).toEqual([{ ptyId: "pty-a", dataLength: 1 }]);
  });

  it("keeps burst and spaced keys ordered across writes", async () => {
    const writes: string[] = [];
    const completions: Array<() => void> = [];
    const started: Array<() => void> = [];
    const queue = new PtyInputWriteQueue((_ptyId, data) => {
      writes.push(data);
      return new Promise<void>((resolve) => {
        completions.push(resolve);
        started.shift()?.();
      });
    }, () => {});
    const waitForWrite = () => new Promise<void>((resolve) => started.push(resolve));

    const firstStarted = waitForWrite();
    const drained = queue.enqueue("pty-a", "a");
    await firstStarted;
    for (const key of ["b", "c"]) queue.enqueue("pty-a", key);
    completions.shift()?.();

    const secondStarted = waitForWrite();
    await secondStarted;
    for (const key of ["d", "e"]) queue.enqueue("pty-a", key);
    completions.shift()?.();

    const thirdStarted = waitForWrite();
    await thirdStarted;
    queue.enqueue("pty-a", "f");
    await Promise.resolve();
    queue.enqueue("pty-a", "g");
    completions.shift()?.();
    const fourthStarted = waitForWrite();
    await fourthStarted;
    completions.shift()?.();
    await drained;

    expect(writes).toEqual(["a", "bc", "de", "fg"]);
  });

  it("keeps each PTY ordered when independent mocked writes resolve out of order", async () => {
    const writes: Array<{ ptyId: string; data: string; resolve: () => void }> = [];
    const started: Array<() => void> = [];
    const queue = new PtyInputWriteQueue((ptyId, data) => new Promise<void>((resolve) => {
      writes.push({ ptyId, data, resolve });
      started.shift()?.();
    }), () => {});
    const waitForWrite = () => new Promise<void>((resolve) => started.push(resolve));
    const release = (ptyId: string, data: string) => {
      const write = writes.find((candidate) => candidate.ptyId === ptyId && candidate.data === data);
      if (!write) throw new Error(`missing write ${ptyId}:${data}`);
      write.resolve();
    };

    const firstStarted = waitForWrite();
    const first = queue.enqueue("pty-a", "a");
    await firstStarted;
    queue.enqueue("pty-a", "b");
    queue.enqueue("pty-a", "c");

    const otherStarted = waitForWrite();
    const other = queue.enqueue("pty-b", "x");
    await otherStarted;
    release("pty-b", "x");
    await other;
    release("pty-a", "a");

    const nextStarted = waitForWrite();
    await nextStarted;
    release("pty-a", "bc");
    await first;

    expect(writes.map(({ ptyId, data }) => `${ptyId}:${data}`)).toEqual([
      "pty-a:a",
      "pty-b:x",
      "pty-a:bc",
    ]);
  });

  it("keeps completion writes ordered and preserves their composed-text mode", async () => {
    const writes: Array<{ data: string; raw?: boolean }> = [];
    let releaseFirst!: () => void;
    let firstStarted!: () => void;
    const firstWriteStarted = new Promise<void>((resolve) => {
      firstStarted = resolve;
    });
    const firstWrite = new Promise<void>((resolve) => {
      releaseFirst = resolve;
    });
    const queue = new PtyInputWriteQueue(async (_ptyId, data, raw) => {
      writes.push({ data, raw });
      if (data === "a") {
        firstStarted();
        await firstWrite;
      }
    }, () => {});

    const drained = queue.enqueue("pty-a", "a");
    await firstWriteStarted;
    queue.enqueue("pty-a", "completion", false);
    queue.enqueue("pty-a", "b");
    releaseFirst();
    await drained;

    expect(writes).toEqual([
      { data: "a", raw: undefined },
      { data: "completion", raw: false },
      { data: "b", raw: undefined },
    ]);
  });

  it("reports one failure until a write succeeds for that PTY", async () => {
    let shouldFail = true;
    const failures: string[] = [];
    const queue = new PtyInputWriteQueue(async () => {
      if (shouldFail) throw new Error("offline");
    }, (ptyId) => failures.push(ptyId));

    await queue.enqueue("pty-a", "first");
    await queue.enqueue("pty-a", "second");
    expect(failures).toEqual(["pty-a"]);

    shouldFail = false;
    await queue.enqueue("pty-a", "recovered");
    shouldFail = true;
    await queue.enqueue("pty-a", "failed-again");
    expect(failures).toEqual(["pty-a", "pty-a"]);
  });
});

finish();
