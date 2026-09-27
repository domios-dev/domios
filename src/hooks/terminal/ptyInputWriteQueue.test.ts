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
});

finish();
